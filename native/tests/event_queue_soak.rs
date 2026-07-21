use yanxu_platform_native::data::Data;
use yanxu_platform_native::event::{
    EventBatcher, EventKind, EventQueueError, EventQueueMetrics, PlatformEvent,
};

const CYCLES: u32 = 4_096;
const CAPACITY: usize = 64;
const FIRST_POINTER_MOVES: u32 = 64;
const WHEEL_EVENTS: u32 = 16;
const LAST_POINTER_MOVES: u32 = 32;
const ACCEPTED_PER_CYCLE: u32 = FIRST_POINTER_MOVES + WHEEL_EVENTS + 1 + LAST_POINTER_MOVES;
const COALESCED_PER_CYCLE: u32 =
    (FIRST_POINTER_MOVES - 1) + (WHEEL_EVENTS - 1) + (LAST_POINTER_MOVES - 1);
const DRAINED_PER_CYCLE: u32 = 4;

fn batch_events(batch: &Data) -> &[Data] {
    let map = batch.as_map().unwrap();
    let Data::Array(events) = &map["事件"] else {
        panic!("事件批次必须包含数组")
    };
    events
}

fn event_sequence(event: &Data) -> i64 {
    let Some(Data::Integer(sequence)) = event.as_map().unwrap().get("序号") else {
        panic!("事件必须包含整数序号")
    };
    *sequence
}

#[test]
fn deterministic_event_soak_preserves_backpressure_order_and_metrics() {
    let mut batcher = EventBatcher::with_capacity(CAPACITY);
    let mut last_sequence = 0_i64;

    for cycle in 0..CYCLES {
        let window = Some(u64::from(cycle % 8) + 1);
        for index in 0..FIRST_POINTER_MOVES {
            batcher
                .push(
                    PlatformEvent::new(
                        EventKind::PointerMoved,
                        window,
                        f64::from(cycle) + f64::from(index) / 1_000.0,
                    )
                    .with("横坐标", f64::from(index)),
                )
                .unwrap();
        }
        for index in 0..WHEEL_EVENTS {
            batcher
                .push(
                    PlatformEvent::new(
                        EventKind::Wheel,
                        window,
                        f64::from(cycle) + 0.1 + f64::from(index) / 1_000.0,
                    )
                    .with("横滚", 1.0)
                    .with("纵滚", -1.0),
                )
                .unwrap();
        }
        batcher
            .push(PlatformEvent::new(
                EventKind::KeyDown,
                window,
                f64::from(cycle) + 0.2,
            ))
            .unwrap();
        for index in 0..LAST_POINTER_MOVES {
            batcher
                .push(
                    PlatformEvent::new(
                        EventKind::PointerMoved,
                        window,
                        f64::from(cycle) + 0.3 + f64::from(index) / 1_000.0,
                    )
                    .with("横坐标", f64::from(index)),
                )
                .unwrap();
        }

        let before_rejection = batcher.metrics();
        let overflow = (0..=(CAPACITY - before_rejection.queued)).map(|index| {
            PlatformEvent::new(
                EventKind::KeyUp,
                window,
                f64::from(cycle) + 0.5 + f64::from(u32::try_from(index).unwrap()) / 1_000.0,
            )
        });
        assert_eq!(batcher.push_batch(overflow), Err(EventQueueError::Full));
        assert_eq!(
            batcher.metrics(),
            EventQueueMetrics {
                rejected: before_rejection.rejected + 1,
                ..before_rejection
            }
        );

        let batch = batcher.take_data().unwrap();
        assert_eq!(
            batch.as_map().unwrap()["批次"],
            Data::Integer(i64::from(cycle + 1))
        );
        let events = batch_events(&batch);
        assert_eq!(events.len(), DRAINED_PER_CYCLE as usize);
        for event in events {
            let sequence = event_sequence(event);
            assert!(sequence > last_sequence);
            last_sequence = sequence;
        }
        assert!(batcher.is_empty());
    }

    assert_eq!(
        batcher.metrics(),
        EventQueueMetrics {
            capacity: CAPACITY,
            queued: 0,
            high_watermark: DRAINED_PER_CYCLE as usize,
            accepted: u64::from(CYCLES * ACCEPTED_PER_CYCLE),
            coalesced: u64::from(CYCLES * COALESCED_PER_CYCLE),
            rejected: u64::from(CYCLES),
            batches: u64::from(CYCLES),
            drained: u64::from(CYCLES * DRAINED_PER_CYCLE),
        }
    );
    assert_eq!(last_sequence, i64::from(CYCLES * ACCEPTED_PER_CYCLE));
}
