//! 无平台句柄的应用和资源所有权模型。

use crate::event::EventBatcher;
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
    pub ime_allowed: bool,
    pub ime_cursor_area: Option<[f64; 4]>,
    pub ime_purpose: String,
    pub cursor: String,
    pub cursor_visible: bool,
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
            ime_allowed: false,
            ime_cursor_area: None,
            ime_purpose: "普通".to_owned(),
            cursor: "默认".to_owned(),
            cursor_visible: true,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Missing(u64),
    Parent(u64),
    Overflow,
}

impl Display for ModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(id) => write!(formatter, "平台资源 {id} 不存在或已关闭"),
            Self::Parent(id) => write!(formatter, "平台父资源 {id} 类型不允许"),
            Self::Overflow => formatter.write_str("平台资源编号已耗尽"),
        }
    }
}

impl Error for ModelError {}

#[derive(Debug)]
pub struct Model {
    next_id: u64,
    resources: BTreeMap<u64, ResourceNode>,
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
        for closing in &order {
            if let Some(node) = self.resources.remove(closing)
                && let Some(parent) = node.parent
                && let Some(parent_node) = self.resources.get_mut(&parent)
            {
                parent_node.children.remove(closing);
            }
        }
        Ok(order)
    }

    #[must_use]
    pub fn count(&self, kind: ResourceKind) -> usize {
        self.resources
            .values()
            .filter(|node| node.state.kind() == kind)
            .count()
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
}
