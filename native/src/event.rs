//! 统一平台事件与 ABI 批处理。

use crate::data::Data;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const EVENT_MAJOR: i64 = 1;
pub const EVENT_MINOR: i64 = 1;
pub const DEFAULT_EVENT_CAPACITY: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    ApplicationStarted,
    ApplicationActivated,
    ApplicationDeactivated,
    ExitRequested,
    WindowShown,
    WindowHidden,
    WindowCloseRequested,
    WindowClosed,
    WindowMoved,
    WindowResized,
    DpiChanged,
    WindowFocused,
    WindowUnfocused,
    RedrawRequested,
    PointerEntered,
    PointerLeft,
    PointerMoved,
    PointerDown,
    PointerUp,
    PointerCancelled,
    Wheel,
    Gesture,
    KeyDown,
    KeyUp,
    TextInput,
    ImeStarted,
    ImeUpdated,
    ImeCommitted,
    ImeCancelled,
    FileEntered,
    FileHovered,
    FileLeft,
    FileDropped,
    ThemeChanged,
    MonitorsChanged,
    Timer,
}

impl EventKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ApplicationStarted => "应用启动",
            Self::ApplicationActivated => "应用激活",
            Self::ApplicationDeactivated => "应用失活",
            Self::ExitRequested => "退出请求",
            Self::WindowShown => "窗口显示",
            Self::WindowHidden => "窗口隐藏",
            Self::WindowCloseRequested => "关闭请求",
            Self::WindowClosed => "关闭完成",
            Self::WindowMoved => "窗口移动",
            Self::WindowResized => "窗口缩放",
            Self::DpiChanged => "DPI变化",
            Self::WindowFocused => "获得焦点",
            Self::WindowUnfocused => "失去焦点",
            Self::RedrawRequested => "需要重绘",
            Self::PointerEntered => "指针进入",
            Self::PointerLeft => "指针离开",
            Self::PointerMoved => "指针移动",
            Self::PointerDown => "指针按下",
            Self::PointerUp => "指针释放",
            Self::PointerCancelled => "指针取消",
            Self::Wheel => "滚轮",
            Self::Gesture => "手势",
            Self::KeyDown => "按键按下",
            Self::KeyUp => "按键释放",
            Self::TextInput => "文本输入",
            Self::ImeStarted => "IME组合开始",
            Self::ImeUpdated => "IME组合更新",
            Self::ImeCommitted => "IME组合提交",
            Self::ImeCancelled => "IME组合取消",
            Self::FileEntered => "文件拖入",
            Self::FileHovered => "文件悬停",
            Self::FileLeft => "文件离开",
            Self::FileDropped => "文件放下",
            Self::ThemeChanged => "系统主题变化",
            Self::MonitorsChanged => "显示器变化",
            Self::Timer => "计时器",
        }
    }

    const fn coalescing(self) -> Coalescing {
        match self {
            Self::PointerMoved
            | Self::WindowMoved
            | Self::WindowResized
            | Self::RedrawRequested => Coalescing::Latest,
            Self::Wheel => Coalescing::AccumulateWheel,
            _ => Coalescing::Never,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coalescing {
    Never,
    Latest,
    AccumulateWheel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlatformEvent {
    pub sequence: i64,
    pub kind: EventKind,
    pub window: Option<u64>,
    pub time_seconds: f64,
    pub fields: BTreeMap<String, Data>,
}

impl PlatformEvent {
    #[must_use]
    pub fn new(kind: EventKind, window: Option<u64>, time_seconds: f64) -> Self {
        Self {
            sequence: 0,
            kind,
            window,
            time_seconds,
            fields: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<Data>) -> Self {
        self.fields.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn to_data(&self) -> Data {
        let mut map = self.fields.clone();
        map.insert("序号".to_owned(), Data::Integer(self.sequence));
        map.insert("类型".to_owned(), Data::String(self.kind.name().to_owned()));
        map.insert("时间".to_owned(), Data::Number(self.time_seconds));
        map.insert(
            "窗口".to_owned(),
            self.window
                .and_then(|value| i64::try_from(value).ok())
                .map_or(Data::Nil, Data::Integer),
        );
        Data::Map(map)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventQueueError {
    Full,
    InvalidNumber,
}

impl Display for EventQueueError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => formatter.write_str("平台事件队列已满"),
            Self::InvalidNumber => formatter.write_str("平台事件包含无效数值"),
        }
    }
}

impl Error for EventQueueError {}

#[derive(Debug, Clone)]
pub struct EventBatcher {
    capacity: usize,
    next_sequence: i64,
    batch_sequence: i64,
    events: Vec<PlatformEvent>,
}

impl Default for EventBatcher {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_EVENT_CAPACITY)
    }
}

impl EventBatcher {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            next_sequence: 1,
            batch_sequence: 1,
            events: Vec::new(),
        }
    }

    pub fn push(&mut self, mut event: PlatformEvent) -> Result<(), EventQueueError> {
        if !event.time_seconds.is_finite() {
            return Err(EventQueueError::InvalidNumber);
        }
        event.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);

        if let Some(previous) = self.events.last_mut()
            && previous.kind == event.kind
            && previous.window == event.window
        {
            match event.kind.coalescing() {
                Coalescing::Latest => {
                    *previous = event;
                    return Ok(());
                }
                Coalescing::AccumulateWheel => {
                    accumulate_wheel(previous, &event)?;
                    previous.sequence = event.sequence;
                    previous.time_seconds = event.time_seconds;
                    return Ok(());
                }
                Coalescing::Never => {}
            }
        }

        if self.events.len() >= self.capacity {
            return Err(EventQueueError::Full);
        }
        self.events.push(event);
        Ok(())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn take_data(&mut self) -> Option<Data> {
        if self.events.is_empty() {
            return None;
        }
        let sequence = self.batch_sequence;
        self.batch_sequence = self.batch_sequence.saturating_add(1);
        let events = std::mem::take(&mut self.events)
            .into_iter()
            .map(|event| event.to_data())
            .collect();
        Some(Data::map([
            ("协议主", Data::Integer(EVENT_MAJOR)),
            ("协议次", Data::Integer(EVENT_MINOR)),
            ("批次", Data::Integer(sequence)),
            ("事件", Data::Array(events)),
        ]))
    }
}

fn accumulate_wheel(
    previous: &mut PlatformEvent,
    event: &PlatformEvent,
) -> Result<(), EventQueueError> {
    let mut totals = [0.0; 2];
    for (index, name) in ["横滚", "纵滚"].into_iter().enumerate() {
        let old = previous
            .fields
            .get(name)
            .and_then(Data::as_number)
            .unwrap_or(0.0);
        let added = event
            .fields
            .get(name)
            .and_then(Data::as_number)
            .unwrap_or(0.0);
        let total = old + added;
        if !total.is_finite() {
            return Err(EventQueueError::InvalidNumber);
        }
        totals[index] = total;
    }
    for (name, total) in ["横滚", "纵滚"].into_iter().zip(totals) {
        previous.fields.insert(name.to_owned(), Data::Number(total));
    }
    for (name, value) in &event.fields {
        if name != "横滚" && name != "纵滚" {
            previous.fields.insert(name.clone(), value.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(batch: &Data) -> &[Data] {
        let map = batch.as_map().unwrap();
        let Data::Array(events) = &map["事件"] else {
            panic!("batch events must be an array")
        };
        events
    }

    #[test]
    fn includes_every_required_event_name() {
        let kinds = [
            EventKind::ApplicationStarted,
            EventKind::ApplicationActivated,
            EventKind::ApplicationDeactivated,
            EventKind::ExitRequested,
            EventKind::WindowShown,
            EventKind::WindowHidden,
            EventKind::WindowCloseRequested,
            EventKind::WindowClosed,
            EventKind::WindowMoved,
            EventKind::WindowResized,
            EventKind::DpiChanged,
            EventKind::WindowFocused,
            EventKind::WindowUnfocused,
            EventKind::RedrawRequested,
            EventKind::PointerEntered,
            EventKind::PointerLeft,
            EventKind::PointerMoved,
            EventKind::PointerDown,
            EventKind::PointerUp,
            EventKind::PointerCancelled,
            EventKind::Wheel,
            EventKind::Gesture,
            EventKind::KeyDown,
            EventKind::KeyUp,
            EventKind::TextInput,
            EventKind::ImeStarted,
            EventKind::ImeUpdated,
            EventKind::ImeCommitted,
            EventKind::ImeCancelled,
            EventKind::FileEntered,
            EventKind::FileHovered,
            EventKind::FileLeft,
            EventKind::FileDropped,
            EventKind::ThemeChanged,
            EventKind::MonitorsChanged,
            EventKind::Timer,
        ];
        assert_eq!(kinds.len(), 36);
        assert!(kinds.iter().all(|kind| !kind.name().is_empty()));
    }

    #[test]
    fn consecutive_pointer_and_resize_events_keep_latest_state() {
        let mut batcher = EventBatcher::default();
        batcher
            .push(PlatformEvent::new(EventKind::PointerMoved, Some(7), 1.0).with("横坐标", 10.0))
            .unwrap();
        batcher
            .push(PlatformEvent::new(EventKind::PointerMoved, Some(7), 2.0).with("横坐标", 20.0))
            .unwrap();
        batcher
            .push(PlatformEvent::new(EventKind::WindowResized, Some(7), 3.0).with("宽", 500.0))
            .unwrap();
        batcher
            .push(PlatformEvent::new(EventKind::WindowResized, Some(7), 4.0).with("宽", 700.0))
            .unwrap();

        let batch = batcher.take_data().unwrap();
        let events = events(&batch);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].as_map().unwrap()["横坐标"], Data::Number(20.0));
        assert_eq!(events[1].as_map().unwrap()["宽"], Data::Number(700.0));
    }

    #[test]
    fn wheel_events_accumulate_per_frame() {
        let mut batcher = EventBatcher::default();
        for (x, y) in [(1.0, 2.0), (-0.5, 3.0)] {
            batcher
                .push(
                    PlatformEvent::new(EventKind::Wheel, Some(4), 1.0)
                        .with("横滚", x)
                        .with("纵滚", y),
                )
                .unwrap();
        }
        let batch = batcher.take_data().unwrap();
        let event = events(&batch)[0].as_map().unwrap();
        assert_eq!(event["横滚"], Data::Number(0.5));
        assert_eq!(event["纵滚"], Data::Number(5.0));
    }

    #[test]
    fn rejected_wheel_accumulation_keeps_the_previous_event_unchanged() {
        let mut batcher = EventBatcher::default();
        batcher
            .push(
                PlatformEvent::new(EventKind::Wheel, Some(4), 1.0)
                    .with("横滚", 1.0)
                    .with("纵滚", f64::MAX),
            )
            .unwrap();
        assert_eq!(
            batcher.push(
                PlatformEvent::new(EventKind::Wheel, Some(4), 2.0)
                    .with("横滚", 2.0)
                    .with("纵滚", f64::MAX),
            ),
            Err(EventQueueError::InvalidNumber)
        );

        let batch = batcher.take_data().unwrap();
        let event = events(&batch)[0].as_map().unwrap();
        assert_eq!(event["横滚"], Data::Number(1.0));
        assert_eq!(event["纵滚"], Data::Number(f64::MAX));
        assert_eq!(event["序号"], Data::Integer(1));
    }

    #[test]
    fn discrete_event_is_a_coalescing_barrier() {
        let mut batcher = EventBatcher::default();
        batcher
            .push(PlatformEvent::new(EventKind::PointerMoved, Some(1), 1.0))
            .unwrap();
        batcher
            .push(PlatformEvent::new(EventKind::PointerDown, Some(1), 2.0))
            .unwrap();
        batcher
            .push(PlatformEvent::new(EventKind::PointerMoved, Some(1), 3.0))
            .unwrap();
        let batch = batcher.take_data().unwrap();
        assert_eq!(events(&batch).len(), 3);
    }

    #[test]
    fn bounded_queue_rejects_discrete_overflow() {
        let mut batcher = EventBatcher::with_capacity(1);
        batcher
            .push(PlatformEvent::new(EventKind::KeyDown, Some(1), 1.0))
            .unwrap();
        assert_eq!(
            batcher.push(PlatformEvent::new(EventKind::KeyUp, Some(1), 2.0)),
            Err(EventQueueError::Full)
        );
    }
}
