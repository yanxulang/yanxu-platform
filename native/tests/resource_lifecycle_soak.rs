use std::time::{Duration, Instant};

use yanxu_platform_native::accessibility::SemanticTree;
use yanxu_platform_native::data::Data;
use yanxu_platform_native::model::{
    AccessibilityMetrics, ApplicationLifecycle, ApplicationLifecycleMetrics, ApplicationState,
    FrameMetrics, Model, ModelError, ResourceKind, ResourceLimits, ResourceMetrics, ResourceState,
    ResourceUsage, TimerState,
};

const CYCLES: u32 = 2_048;
const LIVE_RESOURCES_PER_CYCLE: usize = 11;
const CREATED_RESOURCES_PER_CYCLE: u64 = 15;
const FRAME_BYTES: usize = 96;
const IMAGE_BYTES: usize = 64;
const FONT_BYTES: usize = 32;

fn semantic_tree(name: &str) -> SemanticTree {
    SemanticTree::validate(&Data::map([
        ("编号", Data::Integer(1)),
        ("角色", Data::String("文字".to_owned())),
        ("名称", Data::String(name.to_owned())),
        (
            "边界",
            Data::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
        ),
    ]))
    .unwrap()
}

fn create_window(
    model: &mut Model,
    application: u64,
    first_tree: &SemanticTree,
    second_tree: &SemanticTree,
) -> u64 {
    let window = model
        .create(Some(application), ResourceState::Window(Box::default()))
        .unwrap();
    let first = model.submit_frame(window, vec![1; 64], 1.0).unwrap();
    let second = model
        .submit_frame(window, vec![2; FRAME_BYTES], 2.0)
        .unwrap();
    assert_eq!(first.sequence, 1);
    assert_eq!(first.replaced_sequence, None);
    assert_eq!(second.sequence, 2);
    assert_eq!(second.replaced_sequence, Some(1));
    assert!(
        model
            .replace_accessibility(window, Some(first_tree.clone()))
            .unwrap()
    );
    assert!(
        model
            .replace_accessibility(window, Some(second_tree.clone()))
            .unwrap()
    );
    model.record_accessibility_bridge_activation(window);
    window
}

fn create_timer(model: &mut Model, application: u64, deadline: Instant) -> u64 {
    model
        .create(
            Some(application),
            ResourceState::Timer(TimerState {
                interval: Duration::from_millis(10),
                repeating: true,
                next_deadline: deadline,
                cancelled: false,
            }),
        )
        .unwrap()
}

fn create_image(model: &mut Model, application: u64) -> u64 {
    model
        .create(
            Some(application),
            ResourceState::Image {
                width: 4,
                height: 4,
                rgba: vec![3; IMAGE_BYTES],
            },
        )
        .unwrap()
}

fn create_font(model: &mut Model, application: u64) -> u64 {
    model
        .create(
            Some(application),
            ResourceState::Font {
                family: "浸泡字体".to_owned(),
                bytes: Some(vec![4; FONT_BYTES]),
            },
        )
        .unwrap()
}

#[test]
fn deterministic_resource_lifecycle_soak_reclaims_every_ledger() {
    let limits = ResourceLimits {
        resources: LIVE_RESOURCES_PER_CYCLE,
        windows: 3,
        timers: 3,
        images: 2,
        fonts: 2,
        image_bytes: 2 * IMAGE_BYTES,
        font_bytes: 2 * FONT_BYTES,
        frame_bytes: 3 * FRAME_BYTES,
        accessibility_nodes: 3,
        accessibility_text_bytes: 3 * "乙乙".len(),
    };
    let mut model = Model::with_limits(limits);
    let first_tree = semantic_tree("甲");
    let second_tree = semantic_tree("乙乙");
    let deadline = Instant::now() + Duration::from_secs(60);

    for cycle in 0..CYCLES {
        let application = model
            .create(
                None,
                ResourceState::Application(ApplicationState::new(format!("浸泡-{cycle}"))),
            )
            .unwrap();
        let windows = [
            create_window(&mut model, application, &first_tree, &second_tree),
            create_window(&mut model, application, &first_tree, &second_tree),
            create_window(&mut model, application, &first_tree, &second_tree),
        ];
        let timers = [
            create_timer(&mut model, application, deadline),
            create_timer(&mut model, application, deadline),
            create_timer(&mut model, application, deadline),
        ];
        let images = [
            create_image(&mut model, application),
            create_image(&mut model, application),
        ];
        let fonts = [
            create_font(&mut model, application),
            create_font(&mut model, application),
        ];

        assert_eq!(model.resource_metrics().live, LIVE_RESOURCES_PER_CYCLE);
        assert_eq!(
            model.resource_usage(),
            ResourceUsage {
                resources: LIVE_RESOURCES_PER_CYCLE,
                windows: 3,
                timers: 3,
                images: 2,
                fonts: 2,
                image_bytes: 2 * IMAGE_BYTES,
                font_bytes: 2 * FONT_BYTES,
                frame_bytes: 3 * FRAME_BYTES,
                accessibility_nodes: 3,
                accessibility_text_bytes: 3 * "乙乙".len(),
            }
        );

        model.begin_application_run(application).unwrap();
        assert_eq!(model.close(windows[0]).unwrap(), vec![windows[0]]);
        assert_eq!(model.close(timers[0]).unwrap(), vec![timers[0]]);
        assert_eq!(model.close(images[0]).unwrap(), vec![images[0]]);
        assert_eq!(model.close(fonts[0]).unwrap(), vec![fonts[0]]);

        let replacement_window = create_window(&mut model, application, &first_tree, &second_tree);
        let replacement_timer = create_timer(&mut model, application, deadline);
        let replacement_image = create_image(&mut model, application);
        let replacement_font = create_font(&mut model, application);
        for replacement in [
            replacement_window,
            replacement_timer,
            replacement_image,
            replacement_font,
        ] {
            assert!(model.get(replacement).is_ok());
        }
        assert_eq!(model.resource_metrics().live, LIVE_RESOURCES_PER_CYCLE);
        assert_eq!(
            model.resource_usage(),
            ResourceUsage {
                resources: LIVE_RESOURCES_PER_CYCLE,
                windows: 3,
                timers: 3,
                images: 2,
                fonts: 2,
                image_bytes: 2 * IMAGE_BYTES,
                font_bytes: 2 * FONT_BYTES,
                frame_bytes: 3 * FRAME_BYTES,
                accessibility_nodes: 3,
                accessibility_text_bytes: 3 * "乙乙".len(),
            }
        );

        assert_eq!(model.request_application_exit(application), Ok(true));
        assert_eq!(model.request_application_exit(application), Ok(false));
        let exit_error = (cycle % 2 == 1).then_some("PLATFORM_PRESENT");
        model.finish_application_run(application, exit_error);
        assert_eq!(
            model.application_lifecycle(application),
            ApplicationLifecycle::Exited
        );
        assert_eq!(model.application_exit_error(), exit_error);
        assert_eq!(model.request_application_exit(application), Ok(false));
        assert_eq!(
            model.begin_application_run(application),
            Err(ModelError::ApplicationExited)
        );

        assert_eq!(
            model.close(application).unwrap().len(),
            LIVE_RESOURCES_PER_CYCLE
        );
        assert_eq!(model.resource_usage(), ResourceUsage::default());
        assert_eq!(model.resource_metrics().live, 0);
        assert_eq!(model.frame_metrics().pending, 0);
        assert_eq!(model.accessibility_metrics().current_trees, 0);
        assert_eq!(model.accessibility_metrics().current_nodes, 0);
        assert_eq!(model.accessibility_metrics().current_text_bytes, 0);
        assert_eq!(model.accessibility_metrics().native_bridges_active, 0);
        assert_eq!(
            model.application_lifecycle_summary(),
            ApplicationLifecycle::Closed
        );
        assert_eq!(model.application_exit_error(), None);
        for kind in [
            ResourceKind::Application,
            ResourceKind::Window,
            ResourceKind::Timer,
            ResourceKind::Image,
            ResourceKind::Font,
        ] {
            assert_eq!(model.count(kind), 0);
        }
    }

    let cycles = u64::from(CYCLES);
    let total_resources = cycles * CREATED_RESOURCES_PER_CYCLE;
    assert_eq!(
        model.resource_metrics(),
        ResourceMetrics {
            live: 0,
            high_watermark: LIVE_RESOURCES_PER_CYCLE,
            created: total_resources,
            closed: total_resources,
        }
    );
    assert_eq!(
        model.application_metrics(),
        ApplicationLifecycleMetrics {
            runs_started: cycles,
            exit_requests: cycles,
            duplicate_exit_requests: 2 * cycles,
            exits: cycles,
            normal_exits: cycles / 2,
            failed_exits: cycles / 2,
            closes: cycles,
            resources_reclaimed: cycles * u64::try_from(LIVE_RESOURCES_PER_CYCLE).unwrap(),
            zeroed_closes: cycles,
        }
    );
    assert_eq!(
        model.frame_metrics(),
        FrameMetrics {
            submitted: 8 * cycles,
            replaced: 4 * cycles,
            pending: 0,
            pending_high_watermark: 3,
            bytes_high_watermark: FRAME_BYTES,
            rendered: 0,
            presented: 0,
            failed: 0,
        }
    );
    assert_eq!(
        model.accessibility_metrics(),
        AccessibilityMetrics {
            current_trees: 0,
            current_nodes: 0,
            nodes_high_watermark: 3,
            current_text_bytes: 0,
            text_bytes_high_watermark: 3 * "乙乙".len(),
            updates: 8 * cycles,
            unchanged: 0,
            cleared: 0,
            focus_requests: 0,
            action_requests: 0,
            rejected: 0,
            native_bridges_active: 0,
            native_bridges_high_watermark: 3,
            native_bridge_activations: 4 * cycles,
            native_bridge_deactivations: 4 * cycles,
            native_tree_syncs: 0,
            native_requests: 0,
            native_rejected: 0,
        }
    );
}
