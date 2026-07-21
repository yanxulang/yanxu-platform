//! 无平台句柄的应用和资源所有权模型。

use crate::accessibility::{AccessibilityError, AccessibilitySource, AccessibilityState};
use crate::data::Data;
use crate::event::{EventBatcher, EventKind, EventQueueError, PlatformEvent};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Application,
    Window,
    Timer,
    Image,
    Font,
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
}

impl Display for ModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(id) => write!(formatter, "平台资源 {id} 不存在或已关闭"),
            Self::Parent(id) => write!(formatter, "平台父资源 {id} 类型不允许"),
            Self::Kind(id) => write!(formatter, "平台资源 {id} 类型不允许此操作"),
            Self::FrameSequence => formatter.write_str("平台帧序号已耗尽"),
            Self::Overflow => formatter.write_str("平台资源编号已耗尽"),
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
    pub fn create(&mut self, parent: Option<u64>, state: ResourceState) -> Result<u64, ModelError> {
        if let Some(parent_id) = parent {
            let parent_node = self
                .resources
                .get(&parent_id)
                .ok_or(ModelError::Missing(parent_id))?;
            if !allowed_parent(parent_node.state.kind(), state.kind()) {
                return Err(ModelError::Parent(parent_id));
            }
        } else if state.kind() != ResourceKind::Application {
            return Err(ModelError::Parent(0));
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(ModelError::Overflow)?;
        self.resources.insert(
            id,
            ResourceNode {
                id,
                parent,
                children: BTreeSet::new(),
                state,
            },
        );
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
        for closing in &order {
            if let Some(node) = self.resources.remove(closing)
                && let Some(parent) = node.parent
                && let Some(parent_node) = self.resources.get_mut(&parent)
            {
                parent_node.children.remove(closing);
            }
        }
        self.resource_metrics.live = self.resources.len();
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

    pub fn submit_frame(
        &mut self,
        id: u64,
        frame: Vec<u8>,
        submitted_at_seconds: f64,
    ) -> Result<FrameSubmission, ModelError> {
        let bytes = frame.len();
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
        let outcome: Result<_, AccessibilityModelError> = (|| {
            let node = self.get_mut(window_id)?;
            let ResourceState::Window(window) = &mut node.state else {
                return Err(AccessibilityModelError::from(ModelError::Kind(window_id)));
            };
            let before = (
                usize::from(window.accessibility.tree().is_some()),
                window.accessibility.node_count(),
                window.accessibility.text_bytes(),
            );
            let changed = window.accessibility.replace(tree)?;
            let after = (
                usize::from(window.accessibility.tree().is_some()),
                window.accessibility.node_count(),
                window.accessibility.text_bytes(),
            );
            Ok((changed, before, after))
        })();
        let (changed, before, after) = match outcome {
            Ok(value) => value,
            Err(error) => {
                self.record_accessibility_rejection();
                return Err(error);
            }
        };
        if changed {
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
        let ResourceState::Window(state) = &mut model.get_mut(window).unwrap().state else {
            panic!("window state expected")
        };
        state
            .accessibility
            .replace(Some(SemanticTree::validate(&tree).unwrap()))
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
        let ResourceState::Window(state) = &mut model.get_mut(window).unwrap().state else {
            panic!("window state expected")
        };
        state
            .accessibility
            .replace(Some(SemanticTree::validate(&tree).unwrap()))
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

        assert!(model.replace_accessibility(window, None).unwrap());
        assert_eq!(model.accessibility_metrics().current_trees, 0);
        assert_eq!(model.accessibility_metrics().cleared, 1);
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
