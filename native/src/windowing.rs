//! `winit` 事件循环与 `softbuffer` 绘制表面的真实桌面实现。

use crate::accessibility::AccessibilitySource;
use crate::backend::{HostApi, PlatformResource, monotonic_seconds};
use crate::data::Data;
use crate::event::{EventKind, PlatformEvent};
use crate::model::{DisplayState, Model, ResourceKind, ResourceState, WindowState};
use crate::native_accessibility::{
    NativeActivationHandler, NativeRequest, NativeTreeSnapshot, translate_action_request,
};
use crate::render::{ImageData, RenderEngine};
use crate::sync::RecoverMutex;
use accesskit_winit::{
    Adapter as AccessibilityAdapter, Event as AccessibilityEvent,
    WindowEvent as AccessibilityWindowEvent,
};
use softbuffer::{Context, Surface};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{
    ElementState, Force, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::{Key, ModifiersState, PhysicalKey};
use winit::monitor::MonitorHandle;
use winit::window::{CursorIcon, Fullscreen, ImePurpose, Theme, Window, WindowId, WindowLevel};

struct RunningGuard {
    model: Arc<Mutex<Model>>,
}

enum UserEvent {
    Wake,
    Accessibility(AccessibilityEvent),
}

impl From<AccessibilityEvent> for UserEvent {
    fn from(event: AccessibilityEvent) -> Self {
        Self::Accessibility(event)
    }
}

static PROXIES: std::sync::OnceLock<Mutex<HashMap<u64, EventLoopProxy<UserEvent>>>> =
    std::sync::OnceLock::new();

struct ProxyGuard {
    event_loop_id: u64,
}

impl Drop for ProxyGuard {
    fn drop(&mut self) {
        if let Some(proxies) = PROXIES.get() {
            proxies.lock_recover().remove(&self.event_loop_id);
        }
    }
}

/// 唤醒已运行的 `winit` 循环；找不到循环时返回 `false`。
pub fn wake(event_loop_id: u64) -> bool {
    PROXIES
        .get()
        .and_then(|proxies| proxies.lock_recover().get(&event_loop_id).cloned())
        .is_some_and(|proxy| proxy.send_event(UserEvent::Wake).is_ok())
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.model.lock_recover().running = false;
    }
}

/// 在调用线程运行唯一的桌面事件循环，直到应用请求退出。
pub fn run(
    model: Arc<Mutex<Model>>,
    host: HostApi,
    callback: u64,
    application_id: u64,
) -> Result<(), &'static str> {
    {
        let mut model = model.lock_recover();
        if model.running {
            return Err("PLATFORM_EVENT_LOOP_RUNNING");
        }
        if model.application_exit_requested(application_id).is_none() {
            return Err("PLATFORM_RESOURCE_CLOSED");
        }
        model.running = true;
    }
    let _running = RunningGuard {
        model: model.clone(),
    };
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|_| "PLATFORM_EVENT_LOOP")?;
    let event_loop_proxy = event_loop.create_proxy();
    PROXIES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock_recover()
        .insert(host.0.event_loop_id, event_loop_proxy.clone());
    let _proxy = ProxyGuard {
        event_loop_id: host.0.event_loop_id,
    };
    let context =
        Context::new(event_loop.owned_display_handle()).map_err(|_| "PLATFORM_SURFACE_CREATE")?;
    let mut runner = Runner::new(
        model,
        host,
        callback,
        application_id,
        context,
        event_loop_proxy,
    );
    event_loop
        .run_app(&mut runner)
        .map_err(|_| "PLATFORM_EVENT_LOOP")?;
    runner.fatal.map_or(Ok(()), Err)
}

struct NativeWindow {
    // 表面必须先于其持有的窗口释放。
    surface: Surface<OwnedDisplayHandle, Arc<Window>>,
    // 适配器可能引用原生窗口，必须先于窗口释放。
    accessibility_adapter: AccessibilityAdapter,
    window: Arc<Window>,
    accessibility_snapshot: Arc<Mutex<NativeTreeSnapshot>>,
    accessibility_physical_size: [u32; 2],
    accessibility_scale_factor: f64,
    applied: WindowState,
    modifiers: ModifiersState,
    pointer_position: [f64; 2],
}

struct Runner {
    model: Arc<Mutex<Model>>,
    host: HostApi,
    callback: u64,
    application_id: u64,
    windows: BTreeMap<u64, NativeWindow>,
    window_ids: HashMap<WindowId, u64>,
    context: Context<OwnedDisplayHandle>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
    renderer: RenderEngine,
    loaded_fonts: BTreeSet<u64>,
    focused: HashSet<WindowId>,
    hovered_files: HashSet<WindowId>,
    fatal: Option<&'static str>,
}

impl Runner {
    fn new(
        model: Arc<Mutex<Model>>,
        host: HostApi,
        callback: u64,
        application_id: u64,
        context: Context<OwnedDisplayHandle>,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        Self {
            model,
            host,
            callback,
            application_id,
            windows: BTreeMap::new(),
            window_ids: HashMap::new(),
            context,
            event_loop_proxy,
            renderer: RenderEngine::new(),
            loaded_fonts: BTreeSet::new(),
            focused: HashSet::new(),
            hovered_files: HashSet::new(),
            fatal: None,
        }
    }

    fn fail(&mut self, code: &'static str) {
        self.fatal.get_or_insert(code);
    }

    fn push(&mut self, event: PlatformEvent) {
        if self.fatal.is_some() {
            return;
        }
        if self.model.lock_recover().events.push(event).is_err() {
            self.fail("PLATFORM_QUEUE_FULL");
        }
    }

    fn flush(&mut self) {
        if self.fatal.is_some() {
            return;
        }
        let batch = self.model.lock_recover().events.take_data();
        let Some(batch) = batch else {
            return;
        };
        if let Err(code) = self.host.post(self.callback, batch) {
            self.fail(code);
            return;
        }
        if let Err(code) = self.host.pump() {
            self.fail(code);
        }
    }

    fn application_state(&self) -> Option<bool> {
        self.model
            .lock_recover()
            .application_exit_requested(self.application_id)
    }

    fn process_timers(&mut self) {
        let due = self.model.lock_recover().due_timers(Instant::now());
        for timer in due {
            self.push(
                PlatformEvent::new(EventKind::Timer, None, monotonic_seconds())
                    .with("计时器", i64::try_from(timer).unwrap_or(i64::MAX)),
            );
        }
    }

    fn refresh_environment(&mut self, event_loop: &ActiveEventLoop) {
        let primary = event_loop.primary_monitor();
        let mut displays: Vec<_> = event_loop
            .available_monitors()
            .map(|monitor| display_state(&monitor, primary.as_ref()))
            .collect();
        displays.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.name.cmp(&right.name))
        });
        let theme = theme_name(event_loop.system_theme());
        let (monitors_changed, theme_changed) = {
            let mut model = self.model.lock_recover();
            let monitors_changed = model.displays != displays;
            let theme_changed = model.system_theme != theme;
            model.displays = displays;
            model.system_theme.clone_from(&theme);
            (monitors_changed, theme_changed)
        };
        if monitors_changed {
            self.push(PlatformEvent::new(
                EventKind::MonitorsChanged,
                None,
                monotonic_seconds(),
            ));
        }
        if theme_changed {
            self.push(
                PlatformEvent::new(EventKind::ThemeChanged, None, monotonic_seconds())
                    .with("主题", theme),
            );
        }
    }

    fn sync_model(&mut self, event_loop: &ActiveEventLoop) {
        let Some(exit_requested) = self.application_state() else {
            event_loop.exit();
            return;
        };
        if exit_requested {
            event_loop.exit();
            return;
        }

        let states = self.model.lock_recover().windows();
        let expected: BTreeSet<_> = states.iter().map(|(id, _)| *id).collect();
        let removed: Vec<_> = self
            .windows
            .keys()
            .copied()
            .filter(|id| !expected.contains(id))
            .collect();
        for id in removed {
            if let Some(native) = self.windows.remove(&id) {
                self.window_ids.remove(&native.window.id());
                self.focused.remove(&native.window.id());
                self.hovered_files.remove(&native.window.id());
                self.push(PlatformEvent::new(
                    EventKind::WindowClosed,
                    Some(id),
                    monotonic_seconds(),
                ));
            }
        }

        for (id, state) in &states {
            if self.windows.contains_key(id) {
                continue;
            }
            match self.create_window(event_loop, *id, state.clone()) {
                Ok(native) => {
                    self.window_ids.insert(native.window.id(), *id);
                    self.windows.insert(*id, native);
                }
                Err(code) => {
                    self.fail(code);
                    event_loop.exit();
                    return;
                }
            }
        }

        let mut cleared_redraw = Vec::new();
        for (id, state) in states {
            let Some(native) = self.windows.get_mut(&id) else {
                continue;
            };
            if apply_window_state(native, &state) {
                cleared_redraw.push(id);
            }
        }
        if !cleared_redraw.is_empty() {
            let mut model = self.model.lock_recover();
            for id in cleared_redraw {
                if let Ok(node) = model.get_mut(id)
                    && let ResourceState::Window(window) = &mut node.state
                {
                    window.redraw_requested = false;
                }
            }
        }
    }

    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        model_id: u64,
        mut state: WindowState,
    ) -> Result<NativeWindow, &'static str> {
        let mut attributes = Window::default_attributes()
            .with_title(state.title.clone())
            .with_inner_size(LogicalSize::new(state.width, state.height))
            .with_visible(false)
            .with_transparent(state.transparent)
            .with_decorations(!state.borderless)
            .with_window_level(window_level(state.always_on_top))
            .with_maximized(state.maximized)
            .with_fullscreen(state.fullscreen.then_some(Fullscreen::Borderless(None)));
        if let Some([width, height]) = state.minimum {
            attributes = attributes.with_min_inner_size(LogicalSize::new(width, height));
        }
        if let Some([width, height]) = state.maximum {
            attributes = attributes.with_max_inner_size(LogicalSize::new(width, height));
        }
        if let Some([x, y]) = state.position {
            attributes = attributes.with_position(LogicalPosition::new(x, y));
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|_| "PLATFORM_WINDOW_CREATE")?,
        );
        let surface = Surface::new(&self.context, Arc::clone(&window))
            .map_err(|_| "PLATFORM_SURFACE_CREATE")?;
        let scale = window.scale_factor();
        let inner = window.inner_size();
        let primary = event_loop.primary_monitor();
        let display = window
            .current_monitor()
            .map(|monitor| display_state(&monitor, primary.as_ref()));
        state.width = f64::from(inner.width) / scale;
        state.height = f64::from(inner.height) / scale;
        state.scale_factor = scale;
        state.display.clone_from(&display);
        {
            let mut model = self.model.lock_recover();
            if let Ok(node) = model.get_mut(model_id)
                && let ResourceState::Window(window_state) = &mut node.state
            {
                window_state.width = state.width;
                window_state.height = state.height;
                window_state.scale_factor = scale;
                window_state.display = display;
            }
        }
        let accessibility_physical_size = [inner.width, inner.height];
        let accessibility_snapshot = Arc::new(Mutex::new(NativeTreeSnapshot::new(
            &state.title,
            accessibility_physical_size,
            scale,
            &state.accessibility,
        )));
        let accessibility_adapter = AccessibilityAdapter::with_mixed_handlers(
            event_loop,
            &window,
            NativeActivationHandler::new(Arc::clone(&accessibility_snapshot)),
            self.event_loop_proxy.clone(),
        );
        let applied = WindowState {
            visible: false,
            ..WindowState::default()
        };
        let mut native = NativeWindow {
            surface,
            accessibility_adapter,
            window,
            accessibility_snapshot,
            accessibility_physical_size,
            accessibility_scale_factor: scale,
            applied,
            modifiers: ModifiersState::default(),
            pointer_position: [0.0, 0.0],
        };
        apply_window_state(&mut native, &state);
        Ok(native)
    }

    fn update_window<F>(&self, model_id: u64, update: F)
    where
        F: FnOnce(&mut WindowState),
    {
        let mut model = self.model.lock_recover();
        if let Ok(node) = model.get_mut(model_id)
            && let ResourceState::Window(window) = &mut node.state
        {
            update(window);
        }
    }

    fn update_window_geometry(&self, event_loop: &ActiveEventLoop, model_id: u64) {
        let Some(native) = self.windows.get(&model_id) else {
            return;
        };
        let scale = native.window.scale_factor();
        let size = native.window.inner_size();
        let position = native.window.outer_position().ok();
        let primary = event_loop.primary_monitor();
        let display = native
            .window
            .current_monitor()
            .map(|monitor| display_state(&monitor, primary.as_ref()));
        self.update_window(model_id, |state| {
            state.width = f64::from(size.width) / scale;
            state.height = f64::from(size.height) / scale;
            state.position = position
                .map(|position| [f64::from(position.x) / scale, f64::from(position.y) / scale]);
            state.scale_factor = scale;
            state.display = display;
        });
    }

    fn render(&mut self, model_id: u64) {
        self.sync_fonts();
        let Some(native) = self.windows.get_mut(&model_id) else {
            return;
        };
        let size = native.window.inner_size();
        let (frame, scale, generation, submitted_at_seconds) = {
            let mut model = self.model.lock_recover();
            let Ok(node) = model.get_mut(model_id) else {
                return;
            };
            let ResourceState::Window(window) = &mut node.state else {
                return;
            };
            window.redraw_requested = false;
            (
                window.frame.clone(),
                window.scale_factor,
                window.frame_generation,
                window.frame_submitted_at_seconds,
            )
        };
        if size.width == 0 || size.height == 0 || frame.is_empty() {
            return;
        }
        let host = self.host;
        let rendered =
            match self
                .renderer
                .render(&frame, size.width, size.height, scale as f32, |handle| {
                    image_lookup(host, handle)
                }) {
                Ok(rendered) => rendered,
                Err(_) => {
                    self.model.lock_recover().record_frame_failure();
                    self.fail("PLATFORM_RENDER");
                    return;
                }
            };
        self.model.lock_recover().record_frame_rendered();
        let Some(width) = NonZeroU32::new(rendered.width()) else {
            return;
        };
        let Some(height) = NonZeroU32::new(rendered.height()) else {
            return;
        };
        if native.surface.resize(width, height).is_err() {
            self.model.lock_recover().record_frame_failure();
            self.fail("PLATFORM_PRESENT");
            return;
        }
        let pixels = rendered.xrgb();
        let Ok(mut buffer) = native.surface.buffer_mut() else {
            self.model.lock_recover().record_frame_failure();
            self.fail("PLATFORM_PRESENT");
            return;
        };
        if buffer.len() != pixels.len() {
            self.model.lock_recover().record_frame_failure();
            self.fail("PLATFORM_PRESENT");
            return;
        }
        buffer.copy_from_slice(&pixels);
        if buffer.present().is_err() {
            self.model.lock_recover().record_frame_failure();
            self.fail("PLATFORM_PRESENT");
        } else {
            let presented_at_seconds = monotonic_seconds();
            self.model
                .lock_recover()
                .record_frame_presented(model_id, generation);
            self.push(frame_presented_event(
                model_id,
                generation,
                submitted_at_seconds,
                presented_at_seconds,
            ));
        }
    }

    fn sync_fonts(&mut self) {
        let fonts = self.model.lock_recover().fonts();
        for (id, bytes) in fonts {
            if self.loaded_fonts.insert(id) {
                self.renderer.load_font_data(bytes);
            }
        }
    }

    fn configure_wait(&self, event_loop: &ActiveEventLoop) {
        if self.fatal.is_some() || self.application_state().is_none_or(|exit| exit) {
            event_loop.exit();
            return;
        }
        let deadline = self.model.lock_recover().next_timer_deadline();
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn handle_focus(&mut self, window_id: WindowId, model_id: u64, focused: bool) {
        let was_active = !self.focused.is_empty();
        if focused {
            self.focused.insert(window_id);
        } else {
            self.focused.remove(&window_id);
        }
        let active = !self.focused.is_empty();
        self.push(PlatformEvent::new(
            if focused {
                EventKind::WindowFocused
            } else {
                EventKind::WindowUnfocused
            },
            Some(model_id),
            monotonic_seconds(),
        ));
        if active != was_active {
            self.push(PlatformEvent::new(
                if active {
                    EventKind::ApplicationActivated
                } else {
                    EventKind::ApplicationDeactivated
                },
                None,
                monotonic_seconds(),
            ));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        model_id: u64,
        event: WindowEvent,
    ) {
        let (scale, modifiers, pointer) = self.windows.get(&model_id).map_or(
            (1.0, ModifiersState::default(), [0.0, 0.0]),
            |native| {
                (
                    native.window.scale_factor(),
                    native.modifiers,
                    native.pointer_position,
                )
            },
        );
        match event {
            WindowEvent::Resized(size) => {
                let logical = size.to_logical::<f64>(scale);
                self.update_window(model_id, |window| {
                    window.width = logical.width;
                    window.height = logical.height;
                });
                self.update_window_geometry(event_loop, model_id);
                self.push(
                    PlatformEvent::new(
                        EventKind::WindowResized,
                        Some(model_id),
                        monotonic_seconds(),
                    )
                    .with("宽", logical.width)
                    .with("高", logical.height)
                    .with("物理宽", i64::from(size.width))
                    .with("物理高", i64::from(size.height)),
                );
            }
            WindowEvent::Moved(position) => {
                let logical = position.to_logical::<f64>(scale);
                self.update_window(model_id, |window| {
                    window.position = Some([logical.x, logical.y]);
                });
                self.update_window_geometry(event_loop, model_id);
                self.push(
                    PlatformEvent::new(EventKind::WindowMoved, Some(model_id), monotonic_seconds())
                        .with("横坐标", logical.x)
                        .with("纵坐标", logical.y),
                );
            }
            WindowEvent::CloseRequested => {
                let last_window = self.windows.len() == 1;
                for event in close_request_events(model_id, last_window, monotonic_seconds()) {
                    self.push(event);
                }
            }
            WindowEvent::Destroyed => {
                let _ = self.model.lock_recover().close(model_id);
                if let Some(native) = self.windows.remove(&model_id) {
                    self.window_ids.remove(&native.window.id());
                    self.focused.remove(&native.window.id());
                    self.hovered_files.remove(&native.window.id());
                }
                self.push(PlatformEvent::new(
                    EventKind::WindowClosed,
                    Some(model_id),
                    monotonic_seconds(),
                ));
            }
            WindowEvent::HoveredFile(path) => {
                if self.hovered_files.insert(window_id) {
                    self.push(PlatformEvent::new(
                        EventKind::FileEntered,
                        Some(model_id),
                        monotonic_seconds(),
                    ));
                }
                self.push(
                    PlatformEvent::new(EventKind::FileHovered, Some(model_id), monotonic_seconds())
                        .with("路径", path.to_string_lossy().into_owned()),
                );
            }
            WindowEvent::HoveredFileCancelled => {
                self.hovered_files.remove(&window_id);
                self.push(PlatformEvent::new(
                    EventKind::FileLeft,
                    Some(model_id),
                    monotonic_seconds(),
                ));
            }
            WindowEvent::DroppedFile(path) => {
                self.hovered_files.remove(&window_id);
                self.push(
                    PlatformEvent::new(EventKind::FileDropped, Some(model_id), monotonic_seconds())
                        .with("路径", path.to_string_lossy().into_owned()),
                );
            }
            WindowEvent::Focused(focused) => {
                self.handle_focus(window_id, model_id, focused);
            }
            WindowEvent::ModifiersChanged(value) => {
                if let Some(native) = self.windows.get_mut(&model_id) {
                    native.modifiers = value.state();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                let pressed = event.state == ElementState::Pressed;
                let mut key = PlatformEvent::new(
                    if pressed {
                        EventKind::KeyDown
                    } else {
                        EventKind::KeyUp
                    },
                    Some(model_id),
                    monotonic_seconds(),
                )
                .with("物理键", physical_key_name(&event.physical_key))
                .with("逻辑键", logical_key_name(&event.logical_key))
                .with("位置", format!("{:?}", event.location))
                .with("修饰键", modifiers_data(modifiers))
                .with("重复", event.repeat)
                .with("合成", is_synthetic);
                if let Some(text) = event.text.as_ref() {
                    key = key.with("文本", text.to_string());
                }
                self.push(key);
                if pressed
                    && let Some(text) = event.text
                    && !text.is_empty()
                    && !modifiers.control_key()
                    && !modifiers.super_key()
                {
                    self.push(
                        PlatformEvent::new(
                            EventKind::TextInput,
                            Some(model_id),
                            monotonic_seconds(),
                        )
                        .with("文本", text.to_string()),
                    );
                }
            }
            WindowEvent::Ime(ime) => self.handle_ime(model_id, ime),
            WindowEvent::CursorEntered { device_id } => self.push(
                PlatformEvent::new(
                    EventKind::PointerEntered,
                    Some(model_id),
                    monotonic_seconds(),
                )
                .with("来源", "鼠标")
                .with("设备", format!("{device_id:?}")),
            ),
            WindowEvent::CursorLeft { device_id } => self.push(
                PlatformEvent::new(EventKind::PointerLeft, Some(model_id), monotonic_seconds())
                    .with("来源", "鼠标")
                    .with("设备", format!("{device_id:?}")),
            ),
            WindowEvent::CursorMoved {
                device_id,
                position,
            } => {
                let logical = position.to_logical::<f64>(scale);
                if let Some(native) = self.windows.get_mut(&model_id) {
                    native.pointer_position = [logical.x, logical.y];
                }
                self.push(
                    PlatformEvent::new(
                        EventKind::PointerMoved,
                        Some(model_id),
                        monotonic_seconds(),
                    )
                    .with("来源", "鼠标")
                    .with("设备", format!("{device_id:?}"))
                    .with("横坐标", logical.x)
                    .with("纵坐标", logical.y)
                    .with("修饰键", modifiers_data(modifiers)),
                );
            }
            WindowEvent::MouseInput {
                device_id,
                state,
                button,
            } => self.push(
                PlatformEvent::new(
                    if state == ElementState::Pressed {
                        EventKind::PointerDown
                    } else {
                        EventKind::PointerUp
                    },
                    Some(model_id),
                    monotonic_seconds(),
                )
                .with("来源", "鼠标")
                .with("设备", format!("{device_id:?}"))
                .with("按钮", mouse_button_name(button))
                .with("横坐标", pointer[0])
                .with("纵坐标", pointer[1])
                .with("修饰键", modifiers_data(modifiers)),
            ),
            WindowEvent::MouseWheel {
                device_id,
                delta,
                phase,
            } => {
                let (x, y, unit) = wheel_delta(delta, scale);
                self.push(
                    PlatformEvent::new(EventKind::Wheel, Some(model_id), monotonic_seconds())
                        .with("来源", "触控板或滚轮")
                        .with("设备", format!("{device_id:?}"))
                        .with("横滚", x)
                        .with("纵滚", y)
                        .with("单位", unit)
                        .with("阶段", touch_phase_name(phase))
                        .with("修饰键", modifiers_data(modifiers)),
                );
            }
            WindowEvent::Touch(touch) => {
                let logical = touch.location.to_logical::<f64>(scale);
                let source = if touch.force.as_ref().is_some_and(|force| {
                    matches!(
                        force,
                        Force::Calibrated {
                            altitude_angle: Some(_),
                            ..
                        }
                    )
                }) {
                    "触控笔"
                } else {
                    "触摸"
                };
                let mut pointer_event = PlatformEvent::new(
                    match touch.phase {
                        TouchPhase::Started => EventKind::PointerDown,
                        TouchPhase::Moved => EventKind::PointerMoved,
                        TouchPhase::Ended => EventKind::PointerUp,
                        TouchPhase::Cancelled => EventKind::PointerCancelled,
                    },
                    Some(model_id),
                    monotonic_seconds(),
                )
                .with("来源", source)
                .with("设备", format!("{:?}", touch.device_id))
                .with("指针编号", i64::try_from(touch.id).unwrap_or(i64::MAX))
                .with("横坐标", logical.x)
                .with("纵坐标", logical.y)
                .with("阶段", touch_phase_name(touch.phase))
                .with("修饰键", modifiers_data(modifiers));
                if let Some(force) = touch.force {
                    pointer_event = pointer_event.with("压力", finite(force.normalized()));
                    if let Force::Calibrated {
                        altitude_angle: Some(angle),
                        ..
                    } = force
                    {
                        pointer_event = pointer_event.with("高度角", finite(angle));
                    }
                }
                self.push(pointer_event);
            }
            WindowEvent::PinchGesture {
                device_id,
                delta,
                phase,
            } => self.push(
                gesture_event(model_id, device_id, "缩放", phase).with("变化", finite(delta)),
            ),
            WindowEvent::PanGesture {
                device_id,
                delta,
                phase,
            } => self.push(
                gesture_event(model_id, device_id, "平移", phase)
                    .with("横向变化", finite(f64::from(delta.x) / scale))
                    .with("纵向变化", finite(f64::from(delta.y) / scale)),
            ),
            WindowEvent::DoubleTapGesture { device_id } => self.push(
                PlatformEvent::new(EventKind::Gesture, Some(model_id), monotonic_seconds())
                    .with("设备", format!("{device_id:?}"))
                    .with("手势", "双击"),
            ),
            WindowEvent::RotationGesture {
                device_id,
                delta,
                phase,
            } => self.push(
                gesture_event(model_id, device_id, "旋转", phase)
                    .with("角度", finite(f64::from(delta))),
            ),
            WindowEvent::TouchpadPressure {
                device_id,
                pressure,
                stage,
            } => self.push(
                PlatformEvent::new(EventKind::Gesture, Some(model_id), monotonic_seconds())
                    .with("设备", format!("{device_id:?}"))
                    .with("手势", "压力")
                    .with("压力", finite(f64::from(pressure)))
                    .with("级别", stage),
            ),
            WindowEvent::AxisMotion {
                device_id,
                axis,
                value,
            } => self.push(
                PlatformEvent::new(EventKind::Gesture, Some(model_id), monotonic_seconds())
                    .with("设备", format!("{device_id:?}"))
                    .with("手势", "轴移动")
                    .with("轴", i64::from(axis))
                    .with("值", finite(value)),
            ),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.update_window_geometry(event_loop, model_id);
                self.push(
                    PlatformEvent::new(EventKind::DpiChanged, Some(model_id), monotonic_seconds())
                        .with("比例因子", scale_factor),
                );
                self.refresh_environment(event_loop);
            }
            WindowEvent::ThemeChanged(theme) => {
                let theme = theme_name(Some(theme));
                self.model.lock_recover().system_theme.clone_from(&theme);
                self.push(
                    PlatformEvent::new(
                        EventKind::ThemeChanged,
                        Some(model_id),
                        monotonic_seconds(),
                    )
                    .with("主题", theme),
                );
            }
            WindowEvent::RedrawRequested => {
                self.push(PlatformEvent::new(
                    EventKind::RedrawRequested,
                    Some(model_id),
                    monotonic_seconds(),
                ));
                self.flush();
                self.sync_model(event_loop);
                if self.fatal.is_none() {
                    self.render(model_id);
                }
            }
            WindowEvent::Occluded(_) | WindowEvent::ActivationTokenDone { .. } => {}
        }
        if event_loop.exiting() {
            return;
        }
        if self.fatal.is_some() {
            event_loop.exit();
        }
    }

    fn handle_ime(&mut self, model_id: u64, ime: Ime) {
        self.push(ime_event(model_id, ime, monotonic_seconds()));
    }
}

fn ime_event(model_id: u64, ime: Ime, time_seconds: f64) -> PlatformEvent {
    match ime {
        Ime::Enabled => PlatformEvent::new(EventKind::ImeStarted, Some(model_id), time_seconds),
        Ime::Preedit(text, selection) => {
            let mut event = PlatformEvent::new(EventKind::ImeUpdated, Some(model_id), time_seconds)
                .with("文本", text);
            if let Some((start, end)) = selection {
                event = event
                    .with("选区起", i64::try_from(start).unwrap_or(i64::MAX))
                    .with("选区终", i64::try_from(end).unwrap_or(i64::MAX));
            }
            event
        }
        Ime::Commit(text) => {
            PlatformEvent::new(EventKind::ImeCommitted, Some(model_id), time_seconds)
                .with("文本", text)
        }
        Ime::Disabled => PlatformEvent::new(EventKind::ImeCancelled, Some(model_id), time_seconds),
    }
}

fn close_request_events(model_id: u64, last_window: bool, time_seconds: f64) -> Vec<PlatformEvent> {
    let mut events = vec![PlatformEvent::new(
        EventKind::WindowCloseRequested,
        Some(model_id),
        time_seconds,
    )];
    if last_window {
        events.push(PlatformEvent::new(
            EventKind::ExitRequested,
            None,
            time_seconds,
        ));
    }
    events
}

fn frame_presented_event(
    model_id: u64,
    generation: u64,
    submitted_at_seconds: f64,
    presented_at_seconds: f64,
) -> PlatformEvent {
    let latency_milliseconds = ((presented_at_seconds - submitted_at_seconds).max(0.0)) * 1_000.0;
    PlatformEvent::new(
        EventKind::FramePresented,
        Some(model_id),
        presented_at_seconds,
    )
    .with("帧序号", i64::try_from(generation).unwrap_or(i64::MAX))
    .with("提交时间", submitted_at_seconds)
    .with("呈现时间", presented_at_seconds)
    .with("延迟毫秒", latency_milliseconds)
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.refresh_environment(event_loop);
        self.sync_model(event_loop);
        self.flush();
        self.sync_model(event_loop);
        self.configure_wait(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(model_id) = self.window_ids.get(&window_id).copied() else {
            return;
        };
        if let Some(native) = self.windows.get_mut(&model_id) {
            native
                .accessibility_adapter
                .process_event(&native.window, &event);
        }
        self.handle_window_event(event_loop, window_id, model_id, event);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Accessibility(event) = event else {
            return;
        };
        let Some(model_id) = self.window_ids.get(&event.window_id).copied() else {
            return;
        };
        match event.window_event {
            AccessibilityWindowEvent::ActionRequested(request) => {
                let result = {
                    let mut model = self.model.lock_recover();
                    dispatch_accessibility_request(&mut model, model_id, &request)
                };
                if result == Err("PLATFORM_QUEUE_FULL") {
                    self.fail("PLATFORM_QUEUE_FULL");
                }
            }
            AccessibilityWindowEvent::InitialTreeRequested
            | AccessibilityWindowEvent::AccessibilityDeactivated => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_timers();
        self.refresh_environment(event_loop);
        self.sync_model(event_loop);
        self.flush();
        self.sync_model(event_loop);
        self.configure_wait(event_loop);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.push(PlatformEvent::new(
            EventKind::ApplicationDeactivated,
            None,
            monotonic_seconds(),
        ));
    }
}

fn apply_window_state(native: &mut NativeWindow, state: &WindowState) -> bool {
    let window = &native.window;
    if native.applied.title != state.title {
        window.set_title(&state.title);
    }
    if native.applied.width != state.width || native.applied.height != state.height {
        let _ = window.request_inner_size(LogicalSize::new(state.width, state.height));
    }
    if native.applied.minimum != state.minimum {
        window.set_min_inner_size(
            state
                .minimum
                .map(|[width, height]| LogicalSize::new(width, height)),
        );
    }
    if native.applied.maximum != state.maximum {
        window.set_max_inner_size(
            state
                .maximum
                .map(|[width, height]| LogicalSize::new(width, height)),
        );
    }
    if native.applied.position != state.position
        && let Some([x, y]) = state.position
    {
        window.set_outer_position(LogicalPosition::new(x, y));
    }
    if native.applied.visible != state.visible {
        window.set_visible(state.visible);
    }
    if native.applied.maximized != state.maximized {
        window.set_maximized(state.maximized);
    }
    if native.applied.minimized != state.minimized {
        window.set_minimized(state.minimized);
    }
    if native.applied.fullscreen != state.fullscreen {
        window.set_fullscreen(state.fullscreen.then_some(Fullscreen::Borderless(None)));
    }
    if native.applied.borderless != state.borderless {
        window.set_decorations(!state.borderless);
    }
    if native.applied.transparent != state.transparent {
        window.set_transparent(state.transparent);
    }
    if native.applied.always_on_top != state.always_on_top {
        window.set_window_level(window_level(state.always_on_top));
    }
    if native.applied.ime_allowed != state.ime_allowed {
        window.set_ime_allowed(state.ime_allowed);
    }
    if native.applied.ime_cursor_area != state.ime_cursor_area
        && let Some([x, y, width, height]) = state.ime_cursor_area
    {
        window.set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(width, height));
    }
    if native.applied.ime_purpose != state.ime_purpose {
        window.set_ime_purpose(ime_purpose(&state.ime_purpose));
    }
    if native.applied.cursor != state.cursor {
        window.set_cursor(cursor_icon(&state.cursor));
    }
    if native.applied.cursor_visible != state.cursor_visible {
        window.set_cursor_visible(state.cursor_visible);
    }
    let redraw = state.redraw_requested;
    if redraw {
        window.request_redraw();
    }
    sync_accessibility(native, state);
    native.applied.clone_from(state);
    redraw
}

fn sync_accessibility(native: &mut NativeWindow, state: &WindowState) {
    let inner = native.window.inner_size();
    let physical_size = [inner.width, inner.height];
    let scale_factor = native.window.scale_factor();
    if native.applied.title == state.title
        && native.applied.accessibility == state.accessibility
        && native.accessibility_physical_size == physical_size
        && native.accessibility_scale_factor == scale_factor
    {
        return;
    }
    native.accessibility_physical_size = physical_size;
    native.accessibility_scale_factor = scale_factor;
    native.accessibility_snapshot.lock_recover().replace(
        &state.title,
        physical_size,
        scale_factor,
        &state.accessibility,
    );
    let snapshot = Arc::clone(&native.accessibility_snapshot);
    native
        .accessibility_adapter
        .update_if_active(move || snapshot.lock_recover().full_update());
}

fn dispatch_accessibility_request(
    model: &mut Model,
    window_id: u64,
    request: &accesskit::ActionRequest,
) -> Result<(), &'static str> {
    let translated = (|| {
        let node = model
            .get(window_id)
            .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
        let ResourceState::Window(window) = &node.state else {
            return Err("PLATFORM_RESOURCE_TYPE");
        };
        translate_action_request(&window.accessibility, request)
            .map_err(|_| "PLATFORM_ACCESSIBILITY_ACTION")
    })();
    let translated = match translated {
        Ok(request) => request,
        Err(code) => {
            model.record_accessibility_rejection();
            return Err(code);
        }
    };
    let result = match translated {
        NativeRequest::Focus { node_id } => model.request_accessibility_focus(
            window_id,
            node_id,
            AccessibilitySource::AssistiveTechnology,
            monotonic_seconds(),
        ),
        NativeRequest::Action {
            node_id,
            action,
            argument,
        } => model.request_accessibility_action(
            window_id,
            node_id,
            action,
            argument,
            AccessibilitySource::AssistiveTechnology,
            monotonic_seconds(),
        ),
    };
    result.map_err(|error| error.code())
}

fn display_state(monitor: &MonitorHandle, primary: Option<&MonitorHandle>) -> DisplayState {
    let position = monitor.position();
    let size = monitor.size();
    DisplayState {
        name: monitor.name(),
        position: [position.x, position.y],
        size: [size.width, size.height],
        scale_factor: monitor.scale_factor(),
        primary: primary == Some(monitor),
    }
}

fn theme_name(theme: Option<Theme>) -> String {
    match theme {
        Some(Theme::Light) => "浅色",
        Some(Theme::Dark) => "深色",
        None => "系统",
    }
    .to_owned()
}

const fn window_level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

fn ime_purpose(name: &str) -> ImePurpose {
    match name {
        "密码" => ImePurpose::Password,
        "终端" => ImePurpose::Terminal,
        _ => ImePurpose::Normal,
    }
}

fn cursor_icon(name: &str) -> CursorIcon {
    match name {
        "链接" | "手型" => CursorIcon::Pointer,
        "文本" => CursorIcon::Text,
        "十字" => CursorIcon::Crosshair,
        "等待" => CursorIcon::Wait,
        "进度" => CursorIcon::Progress,
        "移动" => CursorIcon::Move,
        "禁止" => CursorIcon::NotAllowed,
        "抓取" => CursorIcon::Grab,
        "正在抓取" => CursorIcon::Grabbing,
        "水平调整" => CursorIcon::EwResize,
        "垂直调整" => CursorIcon::NsResize,
        "东北西南调整" => CursorIcon::NeswResize,
        "西北东南调整" => CursorIcon::NwseResize,
        _ => CursorIcon::Default,
    }
}

fn physical_key_name(key: &PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(code) => format!("{code:?}"),
    }
}

fn logical_key_name(key: &Key) -> String {
    match key {
        Key::Character(character) => character.to_string(),
        Key::Named(named) => format!("{named:?}"),
        Key::Dead(character) => {
            character.map_or_else(|| "Dead".to_owned(), |value| value.to_string())
        }
        Key::Unidentified(key) => format!("{key:?}"),
    }
}

fn modifiers_data(modifiers: ModifiersState) -> Data {
    Data::map([
        ("Shift", Data::Bool(modifiers.shift_key())),
        ("Control", Data::Bool(modifiers.control_key())),
        ("Alt", Data::Bool(modifiers.alt_key())),
        ("Super", Data::Bool(modifiers.super_key())),
        (
            "主修饰",
            Data::Bool(if cfg!(target_os = "macos") {
                modifiers.super_key()
            } else {
                modifiers.control_key()
            }),
        ),
    ])
}

fn mouse_button_name(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "左".to_owned(),
        MouseButton::Right => "右".to_owned(),
        MouseButton::Middle => "中".to_owned(),
        MouseButton::Back => "后退".to_owned(),
        MouseButton::Forward => "前进".to_owned(),
        MouseButton::Other(value) => format!("其他{value}"),
    }
}

fn wheel_delta(delta: MouseScrollDelta, scale: f64) -> (f64, f64, &'static str) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (finite(f64::from(x)), finite(f64::from(y)), "行"),
        MouseScrollDelta::PixelDelta(position) => (
            finite(position.x / scale),
            finite(position.y / scale),
            "逻辑像素",
        ),
    }
}

const fn touch_phase_name(phase: TouchPhase) -> &'static str {
    match phase {
        TouchPhase::Started => "开始",
        TouchPhase::Moved => "更新",
        TouchPhase::Ended => "结束",
        TouchPhase::Cancelled => "取消",
    }
}

fn gesture_event(
    model_id: u64,
    device_id: winit::event::DeviceId,
    name: &'static str,
    phase: TouchPhase,
) -> PlatformEvent {
    PlatformEvent::new(EventKind::Gesture, Some(model_id), monotonic_seconds())
        .with("设备", format!("{device_id:?}"))
        .with("手势", name)
        .with("阶段", touch_phase_name(phase))
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn image_lookup(host: HostApi, handle: u64) -> Option<ImageData> {
    let raw = host.raw_resource(handle).ok()?;
    // `resource_get` 已限定为当前扩展；所有注册资源均由 `PlatformResource` 包装。
    let resource = unsafe { &*raw.cast::<PlatformResource>() };
    if resource.kind != ResourceKind::Image {
        return None;
    }
    let model = resource.model.lock().ok()?;
    let node = model.get(resource.id).ok()?;
    let ResourceState::Image {
        width,
        height,
        rgba,
    } = &node.state
    else {
        return None;
    };
    Some(ImageData {
        width: *width,
        height: *height,
        rgba: rgba.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::SemanticTree;
    use crate::event::EventBatcher;
    use accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId};
    use winit::event::MouseScrollDelta;
    use winit::keyboard::{KeyCode, NamedKey};

    #[test]
    fn exposes_stable_key_names() {
        assert_eq!(physical_key_name(&PhysicalKey::Code(KeyCode::KeyA)), "KeyA");
        assert_eq!(logical_key_name(&Key::Named(NamedKey::Enter)), "Enter");
        assert_eq!(logical_key_name(&Key::Character("言".into())), "言");
    }

    #[test]
    fn normalizes_pixel_wheel_delta_to_logical_units() {
        let (x, y, unit) = wheel_delta(
            MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(20.0, -10.0)),
            2.0,
        );
        assert_eq!((x, y, unit), (10.0, -5.0, "逻辑像素"));
    }

    #[test]
    fn maps_ime_purpose_and_cursor_without_platform_handles() {
        assert_eq!(ime_purpose("密码"), ImePurpose::Password);
        assert_eq!(ime_purpose("普通"), ImePurpose::Normal);
        assert_eq!(cursor_icon("文本"), CursorIcon::Text);
        assert_eq!(cursor_icon("未知"), CursorIcon::Default);
    }

    #[test]
    fn maps_chinese_ime_preedit_commit_and_lifecycle() {
        let started = ime_event(9, Ime::Enabled, 1.0);
        assert_eq!(started.kind, EventKind::ImeStarted);

        let updated = ime_event(9, Ime::Preedit("言序".to_owned(), Some((0, 3))), 2.0);
        assert_eq!(updated.kind, EventKind::ImeUpdated);
        assert_eq!(updated.fields["文本"], Data::String("言序".to_owned()));
        assert_eq!(updated.fields["选区起"], Data::Integer(0));
        assert_eq!(updated.fields["选区终"], Data::Integer(3));

        let committed = ime_event(9, Ime::Commit("言序".to_owned()), 3.0);
        assert_eq!(committed.kind, EventKind::ImeCommitted);
        assert_eq!(committed.fields["文本"], Data::String("言序".to_owned()));

        let cancelled = ime_event(9, Ime::Disabled, 4.0);
        assert_eq!(cancelled.kind, EventKind::ImeCancelled);
    }

    #[test]
    fn last_window_close_also_requests_application_exit() {
        let last = close_request_events(7, true, 1.0);
        assert_eq!(last.len(), 2);
        assert_eq!(last[0].kind, EventKind::WindowCloseRequested);
        assert_eq!(last[0].window, Some(7));
        assert_eq!(last[1].kind, EventKind::ExitRequested);
        assert_eq!(last[1].window, None);

        let one_of_many = close_request_events(7, false, 1.0);
        assert_eq!(one_of_many.len(), 1);
        assert_eq!(one_of_many[0].kind, EventKind::WindowCloseRequested);
    }

    #[test]
    fn reports_presented_frame_with_monotonic_latency() {
        let event = frame_presented_event(7, 11, 1.25, 1.27).to_data();
        let event = event.as_map().unwrap();
        assert_eq!(event["类型"], Data::String("帧呈现".to_owned()));
        assert_eq!(event["窗口"], Data::Integer(7));
        assert_eq!(event["帧序号"], Data::Integer(11));
        assert_eq!(event["提交时间"], Data::Number(1.25));
        assert_eq!(event["呈现时间"], Data::Number(1.27));
        let Data::Number(latency) = event["延迟毫秒"] else {
            panic!("frame latency must be a number")
        };
        assert!((latency - 20.0).abs() < 1e-9);
    }

    #[test]
    fn native_accessibility_actions_reenter_the_validated_event_queue() {
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
        let tree = Data::map([
            ("编号", Data::Integer(7)),
            ("角色", Data::String("按钮".to_owned())),
            (
                "状态",
                Data::map([("启用", Data::Bool(true)), ("可见", Data::Bool(true))]),
            ),
            ("操作", Data::Array(vec![Data::String("点击".to_owned())])),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 80.into(), 30.into()]),
            ),
        ]);
        let ResourceState::Window(state) = &mut model.get_mut(window).unwrap().state else {
            panic!("window state expected")
        };
        state
            .accessibility
            .replace(Some(SemanticTree::validate(&tree).unwrap()))
            .unwrap();
        let request = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(7),
            data: None,
        };
        dispatch_accessibility_request(&mut model, window, &request).unwrap();
        let batch = model.events.take_data().unwrap();
        let Data::Array(events) = &batch.as_map().unwrap()["事件"] else {
            panic!("events expected")
        };
        let event = events[0].as_map().unwrap();
        assert_eq!(event["类型"], Data::String("无障碍动作请求".to_owned()));
        assert_eq!(event["节点"], Data::Integer(7));
        assert_eq!(event["动作"], Data::String("点击".to_owned()));

        let malformed = ActionRequest {
            data: Some(ActionData::Value("意外参数".into())),
            ..request.clone()
        };
        assert_eq!(
            dispatch_accessibility_request(&mut model, window, &malformed),
            Err("PLATFORM_ACCESSIBILITY_ACTION")
        );
        model.events = EventBatcher::with_capacity(0);
        assert_eq!(
            dispatch_accessibility_request(&mut model, window, &request),
            Err("PLATFORM_QUEUE_FULL")
        );
        assert_eq!(model.accessibility_metrics().action_requests, 1);
        assert_eq!(model.accessibility_metrics().rejected, 2);
    }
}
