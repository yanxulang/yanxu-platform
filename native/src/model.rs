//! 无平台句柄的应用和资源所有权模型。

use crate::accessibility::{AccessibilityError, AccessibilitySource, AccessibilityState};
use crate::data::Data;
use crate::event::{EventBatcher, EventKind, EventQueueError, PlatformEvent};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::{Duration, Instant};

pub const MAX_APPLICATION_RESOURCES: usize = 4_096;
pub const MAX_APPLICATION_WINDOWS: usize = 64;
pub const MAX_APPLICATION_TIMERS: usize = 2_048;
pub const MAX_APPLICATION_IMAGES: usize = 256;
pub const MAX_APPLICATION_FONTS: usize = 64;
pub const MAX_APPLICATION_IMAGE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_APPLICATION_FONT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_APPLICATION_FRAME_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_APPLICATION_ACCESSIBILITY_NODES: usize = 65_536;
pub const MAX_APPLICATION_ACCESSIBILITY_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Application,
    Window,
    Timer,
    Image,
    Font,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub resources: usize,
    pub windows: usize,
    pub timers: usize,
    pub images: usize,
    pub fonts: usize,
    pub image_bytes: usize,
    pub font_bytes: usize,
    pub frame_bytes: usize,
    pub accessibility_nodes: usize,
    pub accessibility_text_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            resources: MAX_APPLICATION_RESOURCES,
            windows: MAX_APPLICATION_WINDOWS,
            timers: MAX_APPLICATION_TIMERS,
            images: MAX_APPLICATION_IMAGES,
            fonts: MAX_APPLICATION_FONTS,
            image_bytes: MAX_APPLICATION_IMAGE_BYTES,
            font_bytes: MAX_APPLICATION_FONT_BYTES,
            frame_bytes: MAX_APPLICATION_FRAME_BYTES,
            accessibility_nodes: MAX_APPLICATION_ACCESSIBILITY_NODES,
            accessibility_text_bytes: MAX_APPLICATION_ACCESSIBILITY_TEXT_BYTES,
        }
    }
}

impl ResourceLimits {
    pub fn validate(self) -> Result<Self, QuotaKind> {
        let maximum = Self::default();
        for (value, limit, kind) in [
            (self.resources, maximum.resources, QuotaKind::Resources),
            (self.windows, maximum.windows, QuotaKind::Windows),
            (self.timers, maximum.timers, QuotaKind::Timers),
            (self.images, maximum.images, QuotaKind::Images),
            (self.fonts, maximum.fonts, QuotaKind::Fonts),
            (self.image_bytes, maximum.image_bytes, QuotaKind::ImageBytes),
            (self.font_bytes, maximum.font_bytes, QuotaKind::FontBytes),
            (self.frame_bytes, maximum.frame_bytes, QuotaKind::FrameBytes),
            (
                self.accessibility_nodes,
                maximum.accessibility_nodes,
                QuotaKind::AccessibilityNodes,
            ),
            (
                self.accessibility_text_bytes,
                maximum.accessibility_text_bytes,
                QuotaKind::AccessibilityTextBytes,
            ),
        ] {
            if value > limit || (kind == QuotaKind::Resources && value == 0) {
                return Err(kind);
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceUsage {
    pub resources: usize,
    pub windows: usize,
    pub timers: usize,
    pub images: usize,
    pub fonts: usize,
    pub image_bytes: usize,
    pub font_bytes: usize,
    pub frame_bytes: usize,
    pub accessibility_nodes: usize,
    pub accessibility_text_bytes: usize,
}

impl ResourceUsage {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            resources: self.resources.checked_add(other.resources)?,
            windows: self.windows.checked_add(other.windows)?,
            timers: self.timers.checked_add(other.timers)?,
            images: self.images.checked_add(other.images)?,
            fonts: self.fonts.checked_add(other.fonts)?,
            image_bytes: self.image_bytes.checked_add(other.image_bytes)?,
            font_bytes: self.font_bytes.checked_add(other.font_bytes)?,
            frame_bytes: self.frame_bytes.checked_add(other.frame_bytes)?,
            accessibility_nodes: self
                .accessibility_nodes
                .checked_add(other.accessibility_nodes)?,
            accessibility_text_bytes: self
                .accessibility_text_bytes
                .checked_add(other.accessibility_text_bytes)?,
        })
    }

    fn checked_sub(self, other: Self) -> Option<Self> {
        Some(Self {
            resources: self.resources.checked_sub(other.resources)?,
            windows: self.windows.checked_sub(other.windows)?,
            timers: self.timers.checked_sub(other.timers)?,
            images: self.images.checked_sub(other.images)?,
            fonts: self.fonts.checked_sub(other.fonts)?,
            image_bytes: self.image_bytes.checked_sub(other.image_bytes)?,
            font_bytes: self.font_bytes.checked_sub(other.font_bytes)?,
            frame_bytes: self.frame_bytes.checked_sub(other.frame_bytes)?,
            accessibility_nodes: self
                .accessibility_nodes
                .checked_sub(other.accessibility_nodes)?,
            accessibility_text_bytes: self
                .accessibility_text_bytes
                .checked_sub(other.accessibility_text_bytes)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaKind {
    Resources,
    Windows,
    Timers,
    Images,
    Fonts,
    ImageBytes,
    FontBytes,
    FrameBytes,
    AccessibilityNodes,
    AccessibilityTextBytes,
}

impl QuotaKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Resources => "PLATFORM_QUOTA_RESOURCES",
            Self::Windows => "PLATFORM_QUOTA_WINDOWS",
            Self::Timers => "PLATFORM_QUOTA_TIMERS",
            Self::Images => "PLATFORM_QUOTA_IMAGES",
            Self::Fonts => "PLATFORM_QUOTA_FONTS",
            Self::ImageBytes => "PLATFORM_QUOTA_IMAGE_BYTES",
            Self::FontBytes => "PLATFORM_QUOTA_FONT_BYTES",
            Self::FrameBytes => "PLATFORM_QUOTA_FRAME_BYTES",
            Self::AccessibilityNodes => "PLATFORM_QUOTA_ACCESSIBILITY_NODES",
            Self::AccessibilityTextBytes => "PLATFORM_QUOTA_ACCESSIBILITY_TEXT_BYTES",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Resources => "资源总数",
            Self::Windows => "窗口数",
            Self::Timers => "计时器数",
            Self::Images => "图片数",
            Self::Fonts => "字体数",
            Self::ImageBytes => "图片字节",
            Self::FontBytes => "字体字节",
            Self::FrameBytes => "帧字节",
            Self::AccessibilityNodes => "无障碍节点",
            Self::AccessibilityTextBytes => "无障碍文字字节",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuotaMetrics {
    pub rejected: u64,
    pub limit_rejected: u64,
    pub configuration_rejected: u64,
    pub locked_rejected: u64,
    pub resources: u64,
    pub windows: u64,
    pub timers: u64,
    pub images: u64,
    pub fonts: u64,
    pub image_bytes: u64,
    pub font_bytes: u64,
    pub frame_bytes: u64,
    pub accessibility_nodes: u64,
    pub accessibility_text_bytes: u64,
}

impl QuotaMetrics {
    fn record_limit(&mut self, kind: QuotaKind) {
        self.rejected = self.rejected.saturating_add(1);
        self.limit_rejected = self.limit_rejected.saturating_add(1);
        let counter = match kind {
            QuotaKind::Resources => &mut self.resources,
            QuotaKind::Windows => &mut self.windows,
            QuotaKind::Timers => &mut self.timers,
            QuotaKind::Images => &mut self.images,
            QuotaKind::Fonts => &mut self.fonts,
            QuotaKind::ImageBytes => &mut self.image_bytes,
            QuotaKind::FontBytes => &mut self.font_bytes,
            QuotaKind::FrameBytes => &mut self.frame_bytes,
            QuotaKind::AccessibilityNodes => &mut self.accessibility_nodes,
            QuotaKind::AccessibilityTextBytes => &mut self.accessibility_text_bytes,
        };
        *counter = counter.saturating_add(1);
    }

    fn record_configuration(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
        self.configuration_rejected = self.configuration_rejected.saturating_add(1);
    }

    fn record_locked(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
        self.locked_rejected = self.locked_rejected.saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub minimum: Option<[f64; 2]>,
    pub maximum: Option<[f64; 2]>,
    pub position: Option<[f64; 2]>,
    pub visible: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub borderless: bool,
    pub transparent: bool,
    pub always_on_top: bool,
    pub scale_factor: f64,
    pub display: Option<DisplayState>,
    pub redraw_requested: bool,
    pub frame: Vec<u8>,
    pub frame_generation: u64,
    pub frame_submitted_at_seconds: f64,
    pub frame_pending: bool,
    pub ime_allowed: bool,
    pub ime_cursor_area: Option<[f64; 4]>,
    pub ime_purpose: String,
    pub cursor: String,
    pub cursor_visible: bool,
    pub accessibility: AccessibilityState,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            title: "言台窗口".to_owned(),
            width: 800.0,
            height: 600.0,
            minimum: None,
            maximum: None,
            position: None,
            visible: true,
            maximized: false,
            minimized: false,
            fullscreen: false,
            borderless: false,
            transparent: false,
            always_on_top: false,
            scale_factor: 1.0,
            display: None,
            redraw_requested: true,
            frame: Vec::new(),
            frame_generation: 0,
            frame_submitted_at_seconds: 0.0,
            frame_pending: false,
            ime_allowed: false,
            ime_cursor_area: None,
            ime_purpose: "普通".to_owned(),
            cursor: "默认".to_owned(),
            cursor_visible: true,
            accessibility: AccessibilityState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimerState {
    pub interval: Duration,
    pub repeating: bool,
    pub next_deadline: Instant,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayState {
    pub name: Option<String>,
    pub position: [i32; 2],
    pub size: [u32; 2],
    pub scale_factor: f64,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceState {
    Application {
        name: String,
        exit_requested: bool,
    },
    Window(Box<WindowState>),
    Timer(TimerState),
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Font {
        family: String,
        bytes: Option<Vec<u8>>,
    },
}

impl ResourceState {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        match self {
            Self::Application { .. } => ResourceKind::Application,
            Self::Window(_) => ResourceKind::Window,
            Self::Timer(_) => ResourceKind::Timer,
            Self::Image { .. } => ResourceKind::Image,
            Self::Font { .. } => ResourceKind::Font,
        }
    }

    fn usage(&self) -> ResourceUsage {
        let mut usage = ResourceUsage {
            resources: 1,
            ..ResourceUsage::default()
        };
        match self {
            Self::Application { .. } => {}
            Self::Window(window) => {
                usage.windows = 1;
                usage.frame_bytes = window.frame.len();
                usage.accessibility_nodes = window.accessibility.node_count();
                usage.accessibility_text_bytes = window.accessibility.text_bytes();
            }
            Self::Timer(_) => usage.timers = 1,
            Self::Image { rgba, .. } => {
                usage.images = 1;
                usage.image_bytes = rgba.len();
            }
            Self::Font { bytes, .. } => {
                usage.fonts = 1;
                usage.font_bytes = bytes.as_ref().map_or(0, Vec::len);
            }
        }
        usage
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub children: BTreeSet<u64>,
    pub state: ResourceState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceMetrics {
    pub live: usize,
    pub high_watermark: usize,
    pub created: u64,
    pub closed: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameMetrics {
    pub submitted: u64,
    pub replaced: u64,
    pub pending: usize,
    pub pending_high_watermark: usize,
    pub bytes_high_watermark: usize,
    pub rendered: u64,
    pub presented: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessibilityMetrics {
    pub current_trees: usize,
    pub current_nodes: usize,
    pub nodes_high_watermark: usize,
    pub current_text_bytes: usize,
    pub text_bytes_high_watermark: usize,
    pub updates: u64,
    pub unchanged: u64,
    pub cleared: u64,
    pub focus_requests: u64,
    pub action_requests: u64,
    pub rejected: u64,
    pub native_bridges_active: usize,
    pub native_bridges_high_watermark: usize,
    pub native_bridge_activations: u64,
    pub native_bridge_deactivations: u64,
    pub native_tree_syncs: u64,
    pub native_requests: u64,
    pub native_rejected: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSubmission {
    pub sequence: u64,
    pub replaced_sequence: Option<u64>,
    pub submitted_at_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Missing(u64),
    Parent(u64),
    Kind(u64),
    FrameSequence,
    Overflow,
    Quota(QuotaKind),
    QuotaConfiguration(QuotaKind),
    QuotaLocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceCreationError {
    Model(ModelError),
    Queue(EventQueueError),
}

impl Display for ResourceCreationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => Display::fmt(error, formatter),
            Self::Queue(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for ResourceCreationError {}

impl From<ModelError> for ResourceCreationError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<EventQueueError> for ResourceCreationError {
    fn from(error: EventQueueError) -> Self {
        Self::Queue(error)
    }
}

impl Display for ModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(id) => write!(formatter, "平台资源 {id} 不存在或已关闭"),
            Self::Parent(id) => write!(formatter, "平台父资源 {id} 类型不允许"),
            Self::Kind(id) => write!(formatter, "平台资源 {id} 类型不允许此操作"),
            Self::FrameSequence => formatter.write_str("平台帧序号已耗尽"),
            Self::Overflow => formatter.write_str("平台资源编号已耗尽"),
            Self::Quota(kind) => write!(formatter, "平台应用{}配额已耗尽", kind.name()),
            Self::QuotaConfiguration(kind) => {
                write!(formatter, "平台应用{}配额配置无效", kind.name())
            }
            Self::QuotaLocked => formatter.write_str("平台应用资源配额已经冻结"),
        }
    }
}

impl Error for ModelError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessibilityModelError {
    Model(ModelError),
    Accessibility(AccessibilityError),
    Queue(EventQueueError),
}

impl AccessibilityModelError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Model(ModelError::Missing(_)) => "PLATFORM_RESOURCE_CLOSED",
            Self::Model(ModelError::Kind(_)) => "PLATFORM_RESOURCE_TYPE",
            Self::Model(ModelError::Quota(kind)) => kind.code(),
            Self::Model(ModelError::QuotaConfiguration(_)) => "PLATFORM_QUOTA_CONFIG",
            Self::Model(ModelError::QuotaLocked) => "PLATFORM_QUOTA_LOCKED",
            Self::Model(_) => "PLATFORM_RESOURCE",
            Self::Accessibility(error) => error.code(),
            Self::Queue(EventQueueError::Full) => "PLATFORM_QUEUE_FULL",
            Self::Queue(EventQueueError::InvalidNumber) => "PLATFORM_VALUE_TYPE",
        }
    }
}

impl Display for AccessibilityModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => Display::fmt(error, formatter),
            Self::Accessibility(error) => Display::fmt(error, formatter),
            Self::Queue(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for AccessibilityModelError {}

impl From<ModelError> for AccessibilityModelError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<AccessibilityError> for AccessibilityModelError {
    fn from(error: AccessibilityError) -> Self {
        Self::Accessibility(error)
    }
}

impl From<EventQueueError> for AccessibilityModelError {
    fn from(error: EventQueueError) -> Self {
        Self::Queue(error)
    }
}

#[derive(Debug)]
pub struct Model {
    next_id: u64,
    resources: BTreeMap<u64, ResourceNode>,
    resource_limits: ResourceLimits,
    resource_limits_locked: bool,
    resource_usage: ResourceUsage,
    quota_metrics: QuotaMetrics,
    resource_metrics: ResourceMetrics,
    frame_metrics: FrameMetrics,
    accessibility_metrics: AccessibilityMetrics,
    active_accessibility_bridges: BTreeSet<u64>,
    pub events: EventBatcher,
    pub running: bool,
    pub displays: Vec<DisplayState>,
    pub system_theme: String,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            next_id: 1,
            resources: BTreeMap::new(),
            resource_limits: ResourceLimits::default(),
            resource_limits_locked: false,
            resource_usage: ResourceUsage::default(),
            quota_metrics: QuotaMetrics::default(),
            resource_metrics: ResourceMetrics::default(),
            frame_metrics: FrameMetrics::default(),
            accessibility_metrics: AccessibilityMetrics::default(),
            active_accessibility_bridges: BTreeSet::new(),
            events: EventBatcher::default(),
            running: false,
            displays: Vec::new(),
            system_theme: "系统".to_owned(),
        }
    }
}

impl Model {
    #[must_use]
    pub fn with_limits(resource_limits: ResourceLimits) -> Self {
        Self {
            resource_limits: resource_limits
                .validate()
                .expect("model limits must not exceed application maxima"),
            ..Self::default()
        }
    }

    pub fn create(&mut self, parent: Option<u64>, state: ResourceState) -> Result<u64, ModelError> {
        let kind = state.kind();
        let next_usage = self.prepare_create(parent, &state)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("preflight resource identifier must remain valid");
        self.resources.insert(
            id,
            ResourceNode {
                id,
                parent,
                children: BTreeSet::new(),
                state,
            },
        );
        self.resource_usage = next_usage;
        if kind != ResourceKind::Application {
            self.resource_limits_locked = true;
        }
        if let Some(parent_id) = parent {
            self.resources
                .get_mut(&parent_id)
                .expect("validated parent disappeared")
                .children
                .insert(id);
        }
        self.resource_metrics.live = self.resources.len();
        self.resource_metrics.high_watermark = self
            .resource_metrics
            .high_watermark
            .max(self.resource_metrics.live);
        self.resource_metrics.created = self.resource_metrics.created.saturating_add(1);
        Ok(id)
    }

    pub fn preflight_create(
        &mut self,
        parent: Option<u64>,
        state: &ResourceState,
    ) -> Result<(), ModelError> {
        self.prepare_create(parent, state).map(drop)
    }

    fn prepare_create(
        &mut self,
        parent: Option<u64>,
        state: &ResourceState,
    ) -> Result<ResourceUsage, ModelError> {
        let kind = state.kind();
        if let Some(parent_id) = parent {
            let parent_node = self
                .resources
                .get(&parent_id)
                .ok_or(ModelError::Missing(parent_id))?;
            if !allowed_parent(parent_node.state.kind(), kind) {
                return Err(ModelError::Parent(parent_id));
            }
        } else if kind != ResourceKind::Application {
            return Err(ModelError::Parent(0));
        }
        let added_usage = state.usage();
        let next_usage = self.checked_resource_usage(added_usage)?;
        self.next_id.checked_add(1).ok_or(ModelError::Overflow)?;
        Ok(next_usage)
    }

    pub fn create_with_events(
        &mut self,
        parent: Option<u64>,
        state: ResourceState,
        events: impl FnOnce(u64) -> Vec<PlatformEvent>,
    ) -> Result<u64, ResourceCreationError> {
        let previous_next_id = self.next_id;
        let previous_limits_locked = self.resource_limits_locked;
        let previous_usage = self.resource_usage;
        let previous_metrics = self.resource_metrics;
        let id = self.create(parent, state)?;
        if let Err(error) = self.events.push_batch(events(id)) {
            let removed = self
                .resources
                .remove(&id)
                .expect("newly created resource must exist during rollback");
            debug_assert_eq!(removed.parent, parent);
            if let Some(parent_id) = parent {
                self.resources
                    .get_mut(&parent_id)
                    .expect("validated parent must exist during rollback")
                    .children
                    .remove(&id);
            }
            self.next_id = previous_next_id;
            self.resource_limits_locked = previous_limits_locked;
            self.resource_usage = previous_usage;
            self.resource_metrics = previous_metrics;
            return Err(error.into());
        }
        Ok(id)
    }

    pub fn get(&self, id: u64) -> Result<&ResourceNode, ModelError> {
        self.resources.get(&id).ok_or(ModelError::Missing(id))
    }

    pub fn get_mut(&mut self, id: u64) -> Result<&mut ResourceNode, ModelError> {
        self.resources.get_mut(&id).ok_or(ModelError::Missing(id))
    }

    #[must_use]
    pub fn windows(&self) -> Vec<(u64, WindowState)> {
        self.resources
            .values()
            .filter_map(|node| match &node.state {
                ResourceState::Window(window) => Some((node.id, window.as_ref().clone())),
                _ => None,
            })
            .collect()
    }

    #[must_use]
    pub fn fonts(&self) -> Vec<(u64, Vec<u8>)> {
        self.resources
            .values()
            .filter_map(|node| match &node.state {
                ResourceState::Font {
                    bytes: Some(bytes), ..
                } => Some((node.id, bytes.clone())),
                _ => None,
            })
            .collect()
    }

    #[must_use]
    pub fn application_exit_requested(&self, id: u64) -> Option<bool> {
        let ResourceState::Application { exit_requested, .. } = &self.get(id).ok()?.state else {
            return None;
        };
        Some(*exit_requested)
    }

    pub fn close(&mut self, id: u64) -> Result<Vec<u64>, ModelError> {
        if !self.resources.contains_key(&id) {
            return Err(ModelError::Missing(id));
        }
        let mut order = Vec::new();
        self.collect_close_order(id, &mut order);
        let pending_frames = order
            .iter()
            .filter(|closing| {
                self.resources.get(closing).is_some_and(|node| {
                    matches!(&node.state, ResourceState::Window(window) if window.frame_pending)
                })
            })
            .count();
        let (accessibility_trees, accessibility_nodes, accessibility_text_bytes) = order
            .iter()
            .filter_map(|closing| {
                let node = self.resources.get(closing)?;
                let ResourceState::Window(window) = &node.state else {
                    return None;
                };
                window.accessibility.tree().map(|_| {
                    (
                        1_usize,
                        window.accessibility.node_count(),
                        window.accessibility.text_bytes(),
                    )
                })
            })
            .fold((0_usize, 0_usize, 0_usize), |totals, values| {
                (
                    totals.0.saturating_add(values.0),
                    totals.1.saturating_add(values.1),
                    totals.2.saturating_add(values.2),
                )
            });
        let active_accessibility_bridges = order
            .iter()
            .filter(|closing| self.active_accessibility_bridges.remove(closing))
            .count();
        let closed_usage = order
            .iter()
            .filter_map(|closing| self.resources.get(closing))
            .fold(ResourceUsage::default(), |total, node| {
                total
                    .checked_add(node.state.usage())
                    .expect("tracked resource usage must not overflow")
            });
        for closing in &order {
            if let Some(node) = self.resources.remove(closing)
                && let Some(parent) = node.parent
                && let Some(parent_node) = self.resources.get_mut(&parent)
            {
                parent_node.children.remove(closing);
            }
        }
        self.resource_metrics.live = self.resources.len();
        self.resource_usage = self
            .resource_usage
            .checked_sub(closed_usage)
            .expect("closed resources must have tracked usage");
        self.resource_metrics.closed = self
            .resource_metrics
            .closed
            .saturating_add(order.len() as u64);
        self.frame_metrics.pending = self.frame_metrics.pending.saturating_sub(pending_frames);
        self.accessibility_metrics.current_trees = self
            .accessibility_metrics
            .current_trees
            .saturating_sub(accessibility_trees);
        self.accessibility_metrics.current_nodes = self
            .accessibility_metrics
            .current_nodes
            .saturating_sub(accessibility_nodes);
        self.accessibility_metrics.current_text_bytes = self
            .accessibility_metrics
            .current_text_bytes
            .saturating_sub(accessibility_text_bytes);
        self.accessibility_metrics.native_bridges_active = self
            .accessibility_metrics
            .native_bridges_active
            .saturating_sub(active_accessibility_bridges);
        self.accessibility_metrics.native_bridge_deactivations = self
            .accessibility_metrics
            .native_bridge_deactivations
            .saturating_add(active_accessibility_bridges as u64);
        Ok(order)
    }

    #[must_use]
    pub fn count(&self, kind: ResourceKind) -> usize {
        self.resources
            .values()
            .filter(|node| node.state.kind() == kind)
            .count()
    }

    #[must_use]
    pub const fn resource_metrics(&self) -> ResourceMetrics {
        self.resource_metrics
    }

    #[must_use]
    pub const fn resource_limits(&self) -> ResourceLimits {
        self.resource_limits
    }

    pub fn configure_resource_limits(
        &mut self,
        limits: ResourceLimits,
    ) -> Result<ResourceLimits, ModelError> {
        let limits = match limits.validate() {
            Ok(limits) => limits,
            Err(kind) => {
                self.quota_metrics.record_configuration();
                return Err(ModelError::QuotaConfiguration(kind));
            }
        };
        if self.resource_limits_locked || self.running {
            self.quota_metrics.record_locked();
            return Err(ModelError::QuotaLocked);
        }
        if let Some(kind) = quota_exceeded(self.resource_usage, limits) {
            self.quota_metrics.record_configuration();
            return Err(ModelError::QuotaConfiguration(kind));
        }
        self.resource_limits = limits;
        Ok(limits)
    }

    pub fn lock_resource_limits(&mut self) {
        self.resource_limits_locked = true;
    }

    #[must_use]
    pub const fn resource_limits_locked(&self) -> bool {
        self.resource_limits_locked
    }

    #[must_use]
    pub const fn resource_usage(&self) -> ResourceUsage {
        self.resource_usage
    }

    #[must_use]
    pub const fn quota_metrics(&self) -> QuotaMetrics {
        self.quota_metrics
    }

    pub fn record_quota_configuration_rejection(&mut self) {
        self.quota_metrics.record_configuration();
    }

    fn checked_resource_usage(
        &mut self,
        added: ResourceUsage,
    ) -> Result<ResourceUsage, ModelError> {
        let Some(next) = self.resource_usage.checked_add(added) else {
            self.quota_metrics.record_limit(QuotaKind::Resources);
            return Err(ModelError::Quota(QuotaKind::Resources));
        };
        if let Some(kind) = quota_exceeded(next, self.resource_limits) {
            self.quota_metrics.record_limit(kind);
            return Err(ModelError::Quota(kind));
        }
        Ok(next)
    }

    pub fn submit_frame(
        &mut self,
        id: u64,
        frame: Vec<u8>,
        submitted_at_seconds: f64,
    ) -> Result<FrameSubmission, ModelError> {
        let bytes = frame.len();
        let previous_bytes = {
            let node = self.get(id)?;
            let ResourceState::Window(window) = &node.state else {
                return Err(ModelError::Kind(id));
            };
            window.frame.len()
        };
        let next_frame_bytes = self
            .resource_usage
            .frame_bytes
            .checked_sub(previous_bytes)
            .and_then(|current| current.checked_add(bytes));
        let Some(next_frame_bytes) = next_frame_bytes else {
            self.quota_metrics.record_limit(QuotaKind::FrameBytes);
            return Err(ModelError::Quota(QuotaKind::FrameBytes));
        };
        if next_frame_bytes > self.resource_limits.frame_bytes {
            self.quota_metrics.record_limit(QuotaKind::FrameBytes);
            return Err(ModelError::Quota(QuotaKind::FrameBytes));
        }
        let (generation, replaced_sequence) = {
            let node = self.get_mut(id)?;
            let ResourceState::Window(window) = &mut node.state else {
                return Err(ModelError::Kind(id));
            };
            let generation = window
                .frame_generation
                .checked_add(1)
                .filter(|value| *value <= i64::MAX as u64)
                .ok_or(ModelError::FrameSequence)?;
            let replaced_sequence = window.frame_pending.then_some(window.frame_generation);
            window.frame = frame;
            window.frame_generation = generation;
            window.frame_submitted_at_seconds = submitted_at_seconds;
            window.frame_pending = true;
            window.redraw_requested = true;
            (generation, replaced_sequence)
        };
        self.resource_usage.frame_bytes = next_frame_bytes;
        self.frame_metrics.submitted = self.frame_metrics.submitted.saturating_add(1);
        self.frame_metrics.bytes_high_watermark =
            self.frame_metrics.bytes_high_watermark.max(bytes);
        if replaced_sequence.is_some() {
            self.frame_metrics.replaced = self.frame_metrics.replaced.saturating_add(1);
        } else {
            self.frame_metrics.pending = self.frame_metrics.pending.saturating_add(1);
            self.frame_metrics.pending_high_watermark = self
                .frame_metrics
                .pending_high_watermark
                .max(self.frame_metrics.pending);
        }
        Ok(FrameSubmission {
            sequence: generation,
            replaced_sequence,
            submitted_at_seconds,
        })
    }

    pub fn record_frame_rendered(&mut self) {
        self.frame_metrics.rendered = self.frame_metrics.rendered.saturating_add(1);
    }

    pub fn record_frame_presented(&mut self, id: u64, generation: u64) {
        self.frame_metrics.presented = self.frame_metrics.presented.saturating_add(1);
        let cleared = self.resources.get_mut(&id).is_some_and(|node| {
            let ResourceState::Window(window) = &mut node.state else {
                return false;
            };
            if window.frame_pending && window.frame_generation == generation {
                window.frame_pending = false;
                true
            } else {
                false
            }
        });
        if cleared {
            self.frame_metrics.pending = self.frame_metrics.pending.saturating_sub(1);
        }
    }

    pub fn record_frame_failure(&mut self) {
        self.frame_metrics.failed = self.frame_metrics.failed.saturating_add(1);
    }

    pub fn replace_accessibility(
        &mut self,
        window_id: u64,
        tree: Option<crate::accessibility::SemanticTree>,
    ) -> Result<bool, AccessibilityModelError> {
        let before: Result<_, AccessibilityModelError> = (|| {
            let node = self.get(window_id)?;
            let ResourceState::Window(window) = &node.state else {
                return Err(ModelError::Kind(window_id).into());
            };
            Ok((
                usize::from(window.accessibility.tree().is_some()),
                window.accessibility.node_count(),
                window.accessibility.text_bytes(),
            ))
        })();
        let before = match before {
            Ok(value) => value,
            Err(error) => {
                self.record_accessibility_rejection();
                return Err(error);
            }
        };
        let after = (
            usize::from(tree.is_some()),
            tree.as_ref().map_or(0, |tree| tree.node_count()),
            tree.as_ref().map_or(0, |tree| tree.text_bytes()),
        );
        let next_nodes = self
            .resource_usage
            .accessibility_nodes
            .checked_sub(before.1)
            .and_then(|current| current.checked_add(after.1));
        let next_nodes = match next_nodes {
            Some(value) if value <= self.resource_limits.accessibility_nodes => value,
            _ => {
                self.record_accessibility_rejection();
                self.quota_metrics
                    .record_limit(QuotaKind::AccessibilityNodes);
                return Err(ModelError::Quota(QuotaKind::AccessibilityNodes).into());
            }
        };
        let next_text_bytes = self
            .resource_usage
            .accessibility_text_bytes
            .checked_sub(before.2)
            .and_then(|current| current.checked_add(after.2));
        let next_text_bytes = match next_text_bytes {
            Some(value) if value <= self.resource_limits.accessibility_text_bytes => value,
            _ => {
                self.record_accessibility_rejection();
                self.quota_metrics
                    .record_limit(QuotaKind::AccessibilityTextBytes);
                return Err(ModelError::Quota(QuotaKind::AccessibilityTextBytes).into());
            }
        };
        let changed = {
            let node = self
                .get_mut(window_id)
                .expect("validated accessibility window disappeared");
            let ResourceState::Window(window) = &mut node.state else {
                unreachable!("validated accessibility resource changed type")
            };
            window.accessibility.replace(tree)
        };
        let changed = match changed {
            Ok(value) => value,
            Err(error) => {
                self.record_accessibility_rejection();
                return Err(error.into());
            }
        };
        if changed {
            self.resource_usage.accessibility_nodes = next_nodes;
            self.resource_usage.accessibility_text_bytes = next_text_bytes;
            let metrics = &mut self.accessibility_metrics;
            metrics.current_trees = metrics
                .current_trees
                .saturating_sub(before.0)
                .saturating_add(after.0);
            metrics.current_nodes = metrics
                .current_nodes
                .saturating_sub(before.1)
                .saturating_add(after.1);
            metrics.current_text_bytes = metrics
                .current_text_bytes
                .saturating_sub(before.2)
                .saturating_add(after.2);
            metrics.nodes_high_watermark = metrics.nodes_high_watermark.max(metrics.current_nodes);
            metrics.text_bytes_high_watermark = metrics
                .text_bytes_high_watermark
                .max(metrics.current_text_bytes);
            metrics.updates = metrics.updates.saturating_add(1);
            if before.0 == 1 && after.0 == 0 {
                metrics.cleared = metrics.cleared.saturating_add(1);
            }
        } else {
            self.accessibility_metrics.unchanged =
                self.accessibility_metrics.unchanged.saturating_add(1);
        }
        Ok(changed)
    }

    pub fn record_accessibility_rejection(&mut self) {
        self.accessibility_metrics.rejected = self.accessibility_metrics.rejected.saturating_add(1);
    }

    pub fn record_accessibility_bridge_activation(&mut self, window_id: u64) {
        let is_window = self
            .resources
            .get(&window_id)
            .is_some_and(|node| matches!(node.state, ResourceState::Window(_)));
        if is_window && self.active_accessibility_bridges.insert(window_id) {
            let metrics = &mut self.accessibility_metrics;
            metrics.native_bridges_active = metrics.native_bridges_active.saturating_add(1);
            metrics.native_bridges_high_watermark = metrics
                .native_bridges_high_watermark
                .max(metrics.native_bridges_active);
            metrics.native_bridge_activations = metrics.native_bridge_activations.saturating_add(1);
        }
    }

    pub fn record_accessibility_bridge_deactivation(&mut self, window_id: u64) {
        if self.active_accessibility_bridges.remove(&window_id) {
            let metrics = &mut self.accessibility_metrics;
            metrics.native_bridges_active = metrics.native_bridges_active.saturating_sub(1);
            metrics.native_bridge_deactivations =
                metrics.native_bridge_deactivations.saturating_add(1);
        }
    }

    pub fn record_accessibility_native_tree_sync(&mut self) {
        self.accessibility_metrics.native_tree_syncs = self
            .accessibility_metrics
            .native_tree_syncs
            .saturating_add(1);
    }

    pub fn record_accessibility_native_request(&mut self) {
        self.accessibility_metrics.native_requests =
            self.accessibility_metrics.native_requests.saturating_add(1);
    }

    pub fn record_accessibility_native_rejection(&mut self) {
        self.accessibility_metrics.native_rejected =
            self.accessibility_metrics.native_rejected.saturating_add(1);
    }

    pub fn request_accessibility_focus(
        &mut self,
        window_id: u64,
        node_id: i64,
        source: AccessibilitySource,
        time_seconds: f64,
    ) -> Result<(), AccessibilityModelError> {
        let result: Result<(), AccessibilityModelError> = (|| {
            let revision = {
                let node = self.get(window_id)?;
                let ResourceState::Window(window) = &node.state else {
                    return Err(ModelError::Kind(window_id).into());
                };
                window.accessibility.focus_target(node_id)?;
                window.accessibility.revision()
            };
            self.events.push(
                PlatformEvent::new(
                    EventKind::AccessibilityFocusRequested,
                    Some(window_id),
                    time_seconds,
                )
                .with("节点", Data::Integer(node_id))
                .with("树修订", Data::Integer(revision))
                .with("来源", source.name()),
            )?;
            Ok(())
        })();
        if result.is_ok() {
            self.accessibility_metrics.focus_requests =
                self.accessibility_metrics.focus_requests.saturating_add(1);
        } else {
            self.record_accessibility_rejection();
        }
        result
    }

    pub fn request_accessibility_action(
        &mut self,
        window_id: u64,
        node_id: i64,
        action: &str,
        argument: Data,
        source: AccessibilitySource,
        time_seconds: f64,
    ) -> Result<(), AccessibilityModelError> {
        let result: Result<(), AccessibilityModelError> = (|| {
            let revision = {
                let node = self.get(window_id)?;
                let ResourceState::Window(window) = &node.state else {
                    return Err(ModelError::Kind(window_id).into());
                };
                window
                    .accessibility
                    .action_target(node_id, action, &argument)?;
                window.accessibility.revision()
            };
            self.events.push(
                PlatformEvent::new(
                    EventKind::AccessibilityActionRequested,
                    Some(window_id),
                    time_seconds,
                )
                .with("节点", Data::Integer(node_id))
                .with("树修订", Data::Integer(revision))
                .with("动作", action)
                .with("参数", argument)
                .with("来源", source.name()),
            )?;
            Ok(())
        })();
        if result.is_ok() {
            self.accessibility_metrics.action_requests =
                self.accessibility_metrics.action_requests.saturating_add(1);
        } else {
            self.record_accessibility_rejection();
        }
        result
    }

    #[must_use]
    pub const fn frame_metrics(&self) -> FrameMetrics {
        self.frame_metrics
    }

    #[must_use]
    pub const fn accessibility_metrics(&self) -> AccessibilityMetrics {
        self.accessibility_metrics
    }

    pub fn due_timers(&mut self, now: Instant) -> Vec<u64> {
        let mut due = Vec::new();
        for node in self.resources.values_mut() {
            let ResourceState::Timer(timer) = &mut node.state else {
                continue;
            };
            if timer.cancelled || timer.next_deadline > now {
                continue;
            }
            due.push(node.id);
            if timer.repeating {
                while timer.next_deadline <= now {
                    timer.next_deadline += timer.interval;
                }
            } else {
                timer.cancelled = true;
            }
        }
        due
    }

    #[must_use]
    pub fn next_timer_deadline(&self) -> Option<Instant> {
        self.resources
            .values()
            .filter_map(|node| {
                let ResourceState::Timer(timer) = &node.state else {
                    return None;
                };
                (!timer.cancelled).then_some(timer.next_deadline)
            })
            .min()
    }

    fn collect_close_order(&self, id: u64, order: &mut Vec<u64>) {
        if let Some(node) = self.resources.get(&id) {
            for child in node.children.iter().rev() {
                self.collect_close_order(*child, order);
            }
            order.push(id);
        }
    }
}

fn quota_exceeded(usage: ResourceUsage, limits: ResourceLimits) -> Option<QuotaKind> {
    [
        (usage.resources, limits.resources, QuotaKind::Resources),
        (usage.windows, limits.windows, QuotaKind::Windows),
        (usage.timers, limits.timers, QuotaKind::Timers),
        (usage.images, limits.images, QuotaKind::Images),
        (usage.fonts, limits.fonts, QuotaKind::Fonts),
        (usage.image_bytes, limits.image_bytes, QuotaKind::ImageBytes),
        (usage.font_bytes, limits.font_bytes, QuotaKind::FontBytes),
        (usage.frame_bytes, limits.frame_bytes, QuotaKind::FrameBytes),
        (
            usage.accessibility_nodes,
            limits.accessibility_nodes,
            QuotaKind::AccessibilityNodes,
        ),
        (
            usage.accessibility_text_bytes,
            limits.accessibility_text_bytes,
            QuotaKind::AccessibilityTextBytes,
        ),
    ]
    .into_iter()
    .find_map(|(value, limit, kind)| (value > limit).then_some(kind))
}

const fn allowed_parent(parent: ResourceKind, child: ResourceKind) -> bool {
    matches!(
        (parent, child),
        (
            ResourceKind::Application,
            ResourceKind::Window | ResourceKind::Timer | ResourceKind::Image | ResourceKind::Font
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::SemanticTree;

    fn app(model: &mut Model) -> u64 {
        model
            .create(
                None,
                ResourceState::Application {
                    name: "测试".to_owned(),
                    exit_requested: false,
                },
            )
            .unwrap()
    }

    #[test]
    fn parent_closes_children_before_itself() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let timer = model
            .create(
                Some(application),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(50),
                    repeating: true,
                    next_deadline: Instant::now() + Duration::from_millis(50),
                    cancelled: false,
                }),
            )
            .unwrap();
        assert_eq!(
            model.close(application).unwrap(),
            vec![timer, window, application]
        );
        assert_eq!(model.count(ResourceKind::Application), 0);
        assert_eq!(model.count(ResourceKind::Window), 0);
        assert_eq!(
            model.resource_metrics(),
            ResourceMetrics {
                live: 0,
                high_watermark: 3,
                created: 3,
                closed: 3,
            }
        );
    }

    #[test]
    fn rejects_invalid_parent_graphs() {
        let mut model = Model::default();
        assert_eq!(
            model.create(None, ResourceState::Window(Box::default()),),
            Err(ModelError::Parent(0))
        );
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        assert_eq!(
            model.create(
                Some(window),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: false,
                    next_deadline: Instant::now() + Duration::from_millis(10),
                    cancelled: false,
                })
            ),
            Err(ModelError::Parent(window))
        );
    }

    #[test]
    fn enforces_resource_count_quotas_before_allocating_ids() {
        let limits = ResourceLimits {
            resources: 3,
            windows: 1,
            timers: 2,
            images: 0,
            fonts: 0,
            image_bytes: 0,
            font_bytes: 0,
            frame_bytes: 0,
            accessibility_nodes: 0,
            accessibility_text_bytes: 0,
        };
        let mut model = Model::with_limits(limits);
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        assert_eq!(
            model.create(Some(application), ResourceState::Window(Box::default())),
            Err(ModelError::Quota(QuotaKind::Windows))
        );
        let timer = model
            .create(
                Some(application),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: false,
                    next_deadline: Instant::now(),
                    cancelled: false,
                }),
            )
            .unwrap();
        assert_eq!(
            model.create(
                Some(application),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: false,
                    next_deadline: Instant::now(),
                    cancelled: false,
                }),
            ),
            Err(ModelError::Quota(QuotaKind::Resources))
        );
        assert_eq!(
            model.resource_usage(),
            ResourceUsage {
                resources: 3,
                windows: 1,
                timers: 1,
                ..ResourceUsage::default()
            }
        );
        assert_eq!(
            model.quota_metrics(),
            QuotaMetrics {
                rejected: 2,
                limit_rejected: 2,
                resources: 1,
                windows: 1,
                ..QuotaMetrics::default()
            }
        );

        model.close(window).unwrap();
        let replacement = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        assert_eq!(replacement, timer + 1);
        model.close(application).unwrap();
        assert_eq!(model.resource_usage(), ResourceUsage::default());
    }

    #[test]
    fn initial_event_failure_rolls_back_resource_creation() {
        let mut model = Model::default();
        let application = app(&mut model);
        model.events = EventBatcher::with_capacity(1);

        assert_eq!(
            model.create_with_events(
                Some(application),
                ResourceState::Window(Box::default()),
                |id| vec![
                    PlatformEvent::new(EventKind::WindowShown, Some(id), 1.0),
                    PlatformEvent::new(EventKind::RedrawRequested, Some(id), 1.0),
                ],
            ),
            Err(ResourceCreationError::Queue(EventQueueError::Full))
        );
        assert_eq!(model.count(ResourceKind::Window), 0);
        assert_eq!(
            model.resource_usage(),
            ResourceUsage {
                resources: 1,
                ..ResourceUsage::default()
            }
        );
        assert_eq!(
            model.resource_metrics(),
            ResourceMetrics {
                live: 1,
                high_watermark: 1,
                created: 1,
                closed: 0,
            }
        );
        assert!(!model.resource_limits_locked());
        assert!(model.get(application).unwrap().children.is_empty());
        assert_eq!(model.events.metrics().queued, 0);
        assert_eq!(model.events.metrics().accepted, 0);
        assert_eq!(model.events.metrics().rejected, 1);

        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        assert_eq!(window, application + 1);
    }

    #[test]
    fn configures_lower_quotas_until_the_first_child_is_created() {
        let mut model = Model::default();
        let application = app(&mut model);
        let limits = ResourceLimits {
            resources: 2,
            windows: 1,
            timers: 0,
            images: 0,
            fonts: 0,
            ..ResourceLimits::default()
        };
        assert_eq!(model.configure_resource_limits(limits), Ok(limits));
        assert_eq!(model.resource_limits(), limits);
        assert!(!model.resource_limits_locked());

        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        assert!(model.resource_limits_locked());
        model.close(window).unwrap();
        assert_eq!(
            model.configure_resource_limits(ResourceLimits::default()),
            Err(ModelError::QuotaLocked)
        );
        assert_eq!(
            model.quota_metrics(),
            QuotaMetrics {
                rejected: 1,
                locked_rejected: 1,
                ..QuotaMetrics::default()
            }
        );
    }

    #[test]
    fn rejects_invalid_or_running_application_quota_configuration() {
        let mut model = Model::default();
        app(&mut model);
        let limits = ResourceLimits {
            resources: 0,
            ..ResourceLimits::default()
        };
        assert_eq!(
            model.configure_resource_limits(limits),
            Err(ModelError::QuotaConfiguration(QuotaKind::Resources))
        );
        let limits = ResourceLimits {
            windows: MAX_APPLICATION_WINDOWS + 1,
            ..ResourceLimits::default()
        };
        assert_eq!(
            model.configure_resource_limits(limits),
            Err(ModelError::QuotaConfiguration(QuotaKind::Windows))
        );

        model.running = true;
        assert_eq!(
            model.configure_resource_limits(ResourceLimits::default()),
            Err(ModelError::QuotaLocked)
        );
        model.running = false;
        model.lock_resource_limits();
        assert_eq!(
            model.configure_resource_limits(ResourceLimits::default()),
            Err(ModelError::QuotaLocked)
        );
        assert_eq!(
            model.quota_metrics(),
            QuotaMetrics {
                rejected: 4,
                configuration_rejected: 2,
                locked_rejected: 2,
                ..QuotaMetrics::default()
            }
        );
    }

    #[test]
    fn rejects_each_resource_kind_at_its_application_quota() {
        let limits = ResourceLimits {
            resources: 8,
            windows: 0,
            timers: 0,
            images: 0,
            fonts: 0,
            image_bytes: 16,
            font_bytes: 16,
            frame_bytes: 16,
            accessibility_nodes: 16,
            accessibility_text_bytes: 16,
        };
        let mut model = Model::with_limits(limits);
        let application = app(&mut model);
        let cases = [
            (ResourceState::Window(Box::default()), QuotaKind::Windows),
            (
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: false,
                    next_deadline: Instant::now(),
                    cancelled: false,
                }),
                QuotaKind::Timers,
            ),
            (
                ResourceState::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                },
                QuotaKind::Images,
            ),
            (
                ResourceState::Font {
                    family: "测试".to_owned(),
                    bytes: Some(vec![0; 4]),
                },
                QuotaKind::Fonts,
            ),
        ];
        for (state, quota) in cases {
            assert_eq!(
                model.create(Some(application), state),
                Err(ModelError::Quota(quota))
            );
            assert!(quota.code().starts_with("PLATFORM_QUOTA_"));
        }
        assert_eq!(
            model.resource_usage(),
            ResourceUsage {
                resources: 1,
                ..ResourceUsage::default()
            }
        );
        assert_eq!(
            model.quota_metrics(),
            QuotaMetrics {
                rejected: 4,
                limit_rejected: 4,
                windows: 1,
                timers: 1,
                images: 1,
                fonts: 1,
                ..QuotaMetrics::default()
            }
        );
    }

    #[test]
    fn releases_owned_image_and_font_byte_quotas_on_close() {
        let limits = ResourceLimits {
            resources: 8,
            windows: 0,
            timers: 0,
            images: 2,
            fonts: 2,
            image_bytes: 4,
            font_bytes: 3,
            frame_bytes: 0,
            accessibility_nodes: 0,
            accessibility_text_bytes: 0,
        };
        let mut model = Model::with_limits(limits);
        let application = app(&mut model);
        let image = model
            .create(
                Some(application),
                ResourceState::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                },
            )
            .unwrap();
        assert_eq!(
            model.create(
                Some(application),
                ResourceState::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 1],
                },
            ),
            Err(ModelError::Quota(QuotaKind::ImageBytes))
        );
        model.close(image).unwrap();
        model
            .create(
                Some(application),
                ResourceState::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 1],
                },
            )
            .unwrap();

        let font = model
            .create(
                Some(application),
                ResourceState::Font {
                    family: "测试".to_owned(),
                    bytes: Some(vec![0; 3]),
                },
            )
            .unwrap();
        assert_eq!(
            model.create(
                Some(application),
                ResourceState::Font {
                    family: "测试二".to_owned(),
                    bytes: Some(vec![0; 1]),
                },
            ),
            Err(ModelError::Quota(QuotaKind::FontBytes))
        );
        model.close(font).unwrap();
        assert_eq!(model.resource_usage().font_bytes, 0);
        assert_eq!(model.quota_metrics().image_bytes, 1);
        assert_eq!(model.quota_metrics().font_bytes, 1);
        model.close(application).unwrap();
        assert_eq!(model.resource_usage(), ResourceUsage::default());
    }

    #[test]
    fn closing_is_explicitly_idempotent_at_resource_wrapper_boundary() {
        let mut model = Model::default();
        let application = app(&mut model);
        assert_eq!(model.close(application).unwrap(), vec![application]);
        assert_eq!(
            model.close(application),
            Err(ModelError::Missing(application))
        );
        assert_eq!(
            model.resource_metrics(),
            ResourceMetrics {
                live: 0,
                high_watermark: 1,
                created: 1,
                closed: 1,
            }
        );
    }

    #[test]
    fn timers_fire_once_or_reschedule_without_drift() {
        let mut model = Model::default();
        let application = app(&mut model);
        let now = Instant::now();
        let once = model
            .create(
                Some(application),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: false,
                    next_deadline: now,
                    cancelled: false,
                }),
            )
            .unwrap();
        let repeating = model
            .create(
                Some(application),
                ResourceState::Timer(TimerState {
                    interval: Duration::from_millis(10),
                    repeating: true,
                    next_deadline: now - Duration::from_millis(25),
                    cancelled: false,
                }),
            )
            .unwrap();
        assert_eq!(model.due_timers(now), vec![once, repeating]);
        assert!(model.due_timers(now).is_empty());
        assert!(
            model
                .next_timer_deadline()
                .is_some_and(|deadline| deadline > now)
        );
    }

    #[test]
    fn exposes_loaded_font_bytes_for_renderer_synchronization() {
        let mut model = Model::default();
        let application = app(&mut model);
        let font = model
            .create(
                Some(application),
                ResourceState::Font {
                    family: "测试字族".to_owned(),
                    bytes: Some(vec![1, 2, 3]),
                },
            )
            .unwrap();
        assert_eq!(model.fonts(), vec![(font, vec![1, 2, 3])]);
        model.close(font).unwrap();
        assert!(model.fonts().is_empty());
    }

    #[test]
    fn replaces_one_pending_frame_and_only_clears_the_current_generation() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let first = model.submit_frame(window, vec![1, 2], 1.25).unwrap();
        let second = model.submit_frame(window, vec![3, 4, 5], 1.5).unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.replaced_sequence, None);
        assert_eq!(first.submitted_at_seconds, 1.25);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.replaced_sequence, Some(1));
        assert_eq!(second.submitted_at_seconds, 1.5);
        assert_eq!(
            model.frame_metrics(),
            FrameMetrics {
                submitted: 2,
                replaced: 1,
                pending: 1,
                pending_high_watermark: 1,
                bytes_high_watermark: 3,
                ..FrameMetrics::default()
            }
        );

        model.record_frame_rendered();
        model.record_frame_presented(window, first.sequence);
        assert_eq!(model.frame_metrics().pending, 1);
        model.record_frame_presented(window, second.sequence);
        assert_eq!(model.frame_metrics().pending, 0);
        assert_eq!(model.frame_metrics().rendered, 1);
        assert_eq!(model.frame_metrics().presented, 2);

        model.submit_frame(window, vec![6], 2.0).unwrap();
        model.record_frame_failure();
        model.close(application).unwrap();
        assert_eq!(model.frame_metrics().pending, 0);
        assert_eq!(model.frame_metrics().failed, 1);
    }

    #[test]
    fn bounds_total_retained_frame_bytes_and_preserves_the_previous_frame() {
        let limits = ResourceLimits {
            resources: 4,
            windows: 2,
            timers: 0,
            images: 0,
            fonts: 0,
            image_bytes: 0,
            font_bytes: 0,
            frame_bytes: 5,
            accessibility_nodes: 0,
            accessibility_text_bytes: 0,
        };
        let mut model = Model::with_limits(limits);
        let application = app(&mut model);
        let first_window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let second_window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();

        model
            .submit_frame(first_window, vec![1, 2, 3, 4], 1.0)
            .unwrap();
        assert_eq!(model.resource_usage().frame_bytes, 4);
        assert_eq!(
            model.submit_frame(second_window, vec![5, 6], 1.1),
            Err(ModelError::Quota(QuotaKind::FrameBytes))
        );
        let ResourceState::Window(second) = &model.get(second_window).unwrap().state else {
            panic!("window state expected")
        };
        assert!(second.frame.is_empty());

        let replacement = model
            .submit_frame(first_window, vec![7, 8, 9, 10, 11], 1.2)
            .unwrap();
        assert_eq!(replacement.sequence, 2);
        assert_eq!(model.resource_usage().frame_bytes, 5);
        assert_eq!(
            model.submit_frame(first_window, vec![0; 6], 1.3),
            Err(ModelError::Quota(QuotaKind::FrameBytes))
        );
        let ResourceState::Window(first) = &model.get(first_window).unwrap().state else {
            panic!("window state expected")
        };
        assert_eq!(first.frame, vec![7, 8, 9, 10, 11]);
        assert_eq!(first.frame_generation, 2);
        assert_eq!(model.frame_metrics().submitted, 2);
        assert_eq!(model.quota_metrics().frame_bytes, 2);

        model.close(first_window).unwrap();
        assert_eq!(model.resource_usage().frame_bytes, 0);
        model.submit_frame(second_window, vec![12; 5], 1.4).unwrap();
        model.close(application).unwrap();
        assert_eq!(model.resource_usage(), ResourceUsage::default());
    }

    #[test]
    fn queues_validated_accessibility_focus_requests_without_mutating_focus() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let tree = Data::map([
            ("编号", Data::Integer(1)),
            ("角色", Data::String("按钮".to_owned())),
            ("名称", Data::String("保存".to_owned())),
            (
                "状态",
                Data::map([
                    ("启用", Data::Bool(true)),
                    ("可见", Data::Bool(true)),
                    ("可聚焦", Data::Bool(true)),
                ]),
            ),
            ("操作", Data::Array(vec![Data::String("聚焦".to_owned())])),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 80.into(), 30.into()]),
            ),
        ]);
        model
            .replace_accessibility(window, Some(SemanticTree::validate(&tree).unwrap()))
            .unwrap();
        model
            .request_accessibility_focus(window, 1, AccessibilitySource::AssistiveTechnology, 2.5)
            .unwrap();
        let batch = model.events.take_data().unwrap();
        let batch = batch.as_map().unwrap();
        let Data::Array(events) = &batch["事件"] else {
            panic!("events expected")
        };
        let event = events[0].as_map().unwrap();
        assert_eq!(event["类型"], Data::String("无障碍焦点请求".to_owned()));
        assert_eq!(event["窗口"], Data::Integer(window as i64));
        assert_eq!(event["节点"], Data::Integer(1));
        assert_eq!(event["树修订"], Data::Integer(1));
        assert_eq!(event["来源"], Data::String("辅助技术".to_owned()));
        assert_eq!(
            model
                .request_accessibility_focus(
                    window,
                    2,
                    AccessibilitySource::AssistiveTechnology,
                    3.0,
                )
                .unwrap_err()
                .code(),
            "PLATFORM_ACCESSIBILITY_NODE"
        );
        assert_eq!(
            model
                .request_accessibility_focus(
                    window,
                    1,
                    AccessibilitySource::AssistiveTechnology,
                    f64::NAN,
                )
                .unwrap_err()
                .code(),
            "PLATFORM_VALUE_TYPE"
        );
        let ResourceState::Window(state) = &model.get(window).unwrap().state else {
            panic!("window state expected")
        };
        assert_eq!(state.accessibility.focused(), None);
        assert!(model.events.is_empty());
        assert_eq!(model.accessibility_metrics().focus_requests, 1);
        assert_eq!(model.accessibility_metrics().rejected, 2);
    }

    #[test]
    fn queues_bounded_accessibility_actions_with_validated_arguments() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let tree = Data::map([
            ("编号", Data::Integer(7)),
            ("角色", Data::String("按钮".to_owned())),
            ("名称", Data::String("保存".to_owned())),
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
        model
            .replace_accessibility(window, Some(SemanticTree::validate(&tree).unwrap()))
            .unwrap();
        model
            .request_accessibility_action(
                window,
                7,
                "点击",
                Data::Nil,
                AccessibilitySource::AssistiveTechnology,
                4.0,
            )
            .unwrap();
        let batch = model.events.take_data().unwrap();
        let batch = batch.as_map().unwrap();
        let Data::Array(events) = &batch["事件"] else {
            panic!("events expected")
        };
        let event = events[0].as_map().unwrap();
        assert_eq!(event["类型"], Data::String("无障碍动作请求".to_owned()));
        assert_eq!(event["节点"], Data::Integer(7));
        assert_eq!(event["树修订"], Data::Integer(1));
        assert_eq!(event["动作"], Data::String("点击".to_owned()));
        assert_eq!(event["参数"], Data::Nil);
        assert_eq!(event["来源"], Data::String("辅助技术".to_owned()));

        assert_eq!(
            model
                .request_accessibility_action(
                    window,
                    7,
                    "点击",
                    Data::Bool(true),
                    AccessibilitySource::AssistiveTechnology,
                    4.5,
                )
                .unwrap_err()
                .code(),
            "PLATFORM_ACCESSIBILITY_ACTION"
        );
        assert!(model.events.is_empty());

        model.events = EventBatcher::with_capacity(0);
        assert_eq!(
            model
                .request_accessibility_action(
                    window,
                    7,
                    "点击",
                    Data::Nil,
                    AccessibilitySource::AssistiveTechnology,
                    5.0,
                )
                .unwrap_err()
                .code(),
            "PLATFORM_QUEUE_FULL"
        );
        assert_eq!(model.accessibility_metrics().action_requests, 1);
        assert_eq!(model.accessibility_metrics().rejected, 2);
    }

    #[test]
    fn tracks_accessibility_tree_watermarks_deduplication_and_cleanup() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let child = Data::map([
            ("编号", Data::Integer(2)),
            ("角色", Data::String("文字".to_owned())),
            ("名称", Data::String("内容".to_owned())),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 80.into(), 20.into()]),
            ),
        ]);
        let tree = Data::map([
            ("编号", Data::Integer(1)),
            ("角色", Data::String("面板".to_owned())),
            ("名称", Data::String("根".to_owned())),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 320.into(), 200.into()]),
            ),
            ("子", Data::Array(vec![child])),
        ]);

        assert!(
            model
                .replace_accessibility(window, Some(SemanticTree::validate(&tree).unwrap()))
                .unwrap()
        );
        assert!(
            !model
                .replace_accessibility(window, Some(SemanticTree::validate(&tree).unwrap()))
                .unwrap()
        );
        let metrics = model.accessibility_metrics();
        assert_eq!(metrics.current_trees, 1);
        assert_eq!(metrics.current_nodes, 2);
        assert_eq!(metrics.nodes_high_watermark, 2);
        assert_eq!(metrics.current_text_bytes, "根内容".len());
        assert_eq!(metrics.text_bytes_high_watermark, "根内容".len());
        assert_eq!(metrics.updates, 1);
        assert_eq!(metrics.unchanged, 1);
        assert_eq!(model.resource_usage().accessibility_nodes, 2);
        assert_eq!(
            model.resource_usage().accessibility_text_bytes,
            "根内容".len()
        );

        assert!(model.replace_accessibility(window, None).unwrap());
        assert_eq!(model.accessibility_metrics().current_trees, 0);
        assert_eq!(model.accessibility_metrics().cleared, 1);
        assert_eq!(model.resource_usage().accessibility_nodes, 0);
        assert_eq!(model.resource_usage().accessibility_text_bytes, 0);
        assert!(
            model
                .replace_accessibility(window, Some(SemanticTree::validate(&tree).unwrap()))
                .unwrap()
        );
        model.close(window).unwrap();
        let metrics = model.accessibility_metrics();
        assert_eq!(metrics.current_trees, 0);
        assert_eq!(metrics.current_nodes, 0);
        assert_eq!(metrics.current_text_bytes, 0);
        assert_eq!(metrics.nodes_high_watermark, 2);
        assert_eq!(metrics.updates, 3);
        assert_eq!(model.resource_usage().accessibility_nodes, 0);
        assert_eq!(model.resource_usage().accessibility_text_bytes, 0);
    }

    #[test]
    fn bounds_accessibility_nodes_and_text_across_application_windows() {
        let limits = ResourceLimits {
            resources: 3,
            windows: 2,
            timers: 0,
            images: 0,
            fonts: 0,
            image_bytes: 0,
            font_bytes: 0,
            frame_bytes: 0,
            accessibility_nodes: 2,
            accessibility_text_bytes: 5,
        };
        let mut model = Model::with_limits(limits);
        let application = app(&mut model);
        let first_window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let second_window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let single = |id, name: &str| {
            SemanticTree::validate(&Data::map([
                ("编号", Data::Integer(id)),
                ("角色", Data::String("文字".to_owned())),
                ("名称", Data::String(name.to_owned())),
                (
                    "边界",
                    Data::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
                ),
            ]))
            .unwrap()
        };
        let double = SemanticTree::validate(&Data::map([
            ("编号", Data::Integer(10)),
            ("角色", Data::String("面板".to_owned())),
            ("名称", Data::String(String::new())),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
            ),
            (
                "子",
                Data::Array(vec![Data::map([
                    ("编号", Data::Integer(11)),
                    ("角色", Data::String("文字".to_owned())),
                    ("名称", Data::String(String::new())),
                    (
                        "边界",
                        Data::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
                    ),
                ])]),
            ),
        ]))
        .unwrap();

        model
            .replace_accessibility(first_window, Some(single(1, "甲")))
            .unwrap();
        assert_eq!(model.resource_usage().accessibility_nodes, 1);
        assert_eq!(model.resource_usage().accessibility_text_bytes, 3);
        assert_eq!(
            model
                .replace_accessibility(second_window, Some(single(2, "乙")))
                .unwrap_err()
                .code(),
            "PLATFORM_QUOTA_ACCESSIBILITY_TEXT_BYTES"
        );
        assert_eq!(
            model
                .replace_accessibility(second_window, Some(double.clone()))
                .unwrap_err()
                .code(),
            "PLATFORM_QUOTA_ACCESSIBILITY_NODES"
        );
        let ResourceState::Window(second) = &model.get(second_window).unwrap().state else {
            panic!("window state expected")
        };
        assert_eq!(second.accessibility.revision(), 0);
        assert!(second.accessibility.tree().is_none());
        assert_eq!(model.accessibility_metrics().rejected, 2);
        assert_eq!(model.quota_metrics().accessibility_nodes, 1);
        assert_eq!(model.quota_metrics().accessibility_text_bytes, 1);
        assert_eq!(model.quota_metrics().limit_rejected, 2);
        assert_eq!(model.resource_usage().accessibility_nodes, 1);
        assert_eq!(model.resource_usage().accessibility_text_bytes, 3);

        model
            .replace_accessibility(first_window, Some(double))
            .unwrap();
        assert_eq!(model.resource_usage().accessibility_nodes, 2);
        assert_eq!(model.resource_usage().accessibility_text_bytes, 0);
        model.close(first_window).unwrap();
        assert_eq!(model.resource_usage().accessibility_nodes, 0);
        model
            .replace_accessibility(second_window, Some(single(2, "乙")))
            .unwrap();
        model.close(application).unwrap();
        assert_eq!(model.resource_usage(), ResourceUsage::default());
    }

    #[test]
    fn tracks_native_accessibility_bridge_lifecycle_and_requests() {
        let mut model = Model::default();
        let application = app(&mut model);
        let first = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let second = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();

        model.record_accessibility_bridge_activation(first);
        model.record_accessibility_bridge_activation(first);
        model.record_accessibility_bridge_activation(second);
        model.record_accessibility_bridge_deactivation(first);
        model.record_accessibility_bridge_deactivation(first);
        model.record_accessibility_native_tree_sync();
        model.record_accessibility_native_request();
        model.record_accessibility_native_rejection();

        let metrics = model.accessibility_metrics();
        assert_eq!(metrics.native_bridges_active, 1);
        assert_eq!(metrics.native_bridges_high_watermark, 2);
        assert_eq!(metrics.native_bridge_activations, 2);
        assert_eq!(metrics.native_bridge_deactivations, 1);
        assert_eq!(metrics.native_tree_syncs, 1);
        assert_eq!(metrics.native_requests, 1);
        assert_eq!(metrics.native_rejected, 1);

        model.close(application).unwrap();
        let metrics = model.accessibility_metrics();
        assert_eq!(metrics.native_bridges_active, 0);
        assert_eq!(metrics.native_bridge_deactivations, 2);
    }

    #[test]
    fn rejects_frame_sequences_that_cannot_cross_the_data_boundary() {
        let mut model = Model::default();
        let application = app(&mut model);
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        let node = model.get_mut(window).unwrap();
        let ResourceState::Window(state) = &mut node.state else {
            panic!("created resource must be a window")
        };
        state.frame_generation = i64::MAX as u64;

        assert_eq!(
            model.submit_frame(window, vec![1], 1.0),
            Err(ModelError::FrameSequence)
        );
        assert_eq!(model.frame_metrics(), FrameMetrics::default());
    }
}
