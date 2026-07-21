use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use yanxu_platform_native::event::{EventBatcher, EventKind, PlatformEvent};
use yanxu_platform_native::model::{
    ApplicationState, Model, ResourceState, TimerState, WindowState,
};
use yanxu_platform_native::protocol::{self, Command, Frame, PayloadWriter};
use yanxu_platform_native::render::{OP_CLEAR, OP_FILL_RECT, RenderEngine};

const SAMPLE_TARGET: Duration = Duration::from_millis(200);
const CALIBRATION_FLOOR: Duration = Duration::from_millis(25);
const WARMUP_SAMPLES: usize = 2;
const MEASURED_SAMPLES: usize = 9;
const MAX_MAD_PERCENT: f64 = 20.0;

struct BenchmarkCase {
    name: &'static str,
    unit: &'static str,
    units_per_iteration: u64,
    budget_nanoseconds_per_unit: f64,
    run: Box<dyn FnMut(u64) -> u64>,
}

struct BenchmarkResult {
    name: &'static str,
    unit: &'static str,
    iterations_per_sample: u64,
    median_nanoseconds_per_unit: f64,
    mad_percent: f64,
    budget_nanoseconds_per_unit: f64,
    passed: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("性能预算必须使用 Cargo release 模式运行".into());
    }
    let report_path = report_path()?;
    let mut results = Vec::new();
    for case in benchmark_cases()? {
        results.push(measure(case));
    }
    let passed = results.iter().all(|result| result.passed);
    let report = report(&results, passed);
    print!("{report}");
    if let Some(path) = report_path {
        fs::write(path, &report)?;
    }
    if !passed {
        return Err("Release 性能预算未通过".into());
    }
    Ok(())
}

fn report_path() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let mut report_path = None;
    while let Some(argument) = arguments.next() {
        if argument != "--report" {
            return Err(format!("未知参数：{}", argument.to_string_lossy()).into());
        }
        let Some(path) = arguments.next() else {
            return Err("--report 必须提供文件路径".into());
        };
        if report_path.replace(PathBuf::from(path)).is_some() {
            return Err("--report 只能提供一次".into());
        }
    }
    Ok(report_path)
}

fn benchmark_cases() -> Result<Vec<BenchmarkCase>, Box<dyn Error>> {
    let protocol_bytes = protocol_frame()?;
    let protocol_commands = u64::try_from(protocol::decode(&protocol_bytes)?.commands.len())?;
    let protocol_case = BenchmarkCase {
        name: "draw_protocol_decode",
        unit: "command",
        units_per_iteration: protocol_commands,
        budget_nanoseconds_per_unit: 200.0,
        run: Box::new(move |iterations| {
            let mut checksum = 0_u64;
            for _ in 0..iterations {
                let frame = protocol::decode(black_box(&protocol_bytes)).unwrap();
                checksum = checksum.wrapping_add(u64::try_from(frame.commands.len()).unwrap());
                black_box(frame);
            }
            checksum
        }),
    };

    let event_case = BenchmarkCase {
        name: "event_coalesce_and_drain",
        unit: "event",
        units_per_iteration: 256,
        budget_nanoseconds_per_unit: 1_000.0,
        run: Box::new(|iterations| {
            let mut checksum = 0_u64;
            for iteration in 0..iterations {
                let mut batcher = EventBatcher::with_capacity(256);
                for index in 0_u32..256 {
                    let kind = if index % 8 == 7 {
                        EventKind::KeyDown
                    } else {
                        EventKind::PointerMoved
                    };
                    batcher
                        .push(
                            PlatformEvent::new(kind, Some(1), iteration as f64)
                                .with("横坐标", f64::from(index)),
                        )
                        .unwrap();
                }
                checksum = checksum.wrapping_add(batcher.metrics().accepted);
                black_box(batcher.take_data().unwrap());
            }
            checksum
        }),
    };

    let deadline = Instant::now() + Duration::from_secs(60);
    let lifecycle_case = BenchmarkCase {
        name: "resource_lifecycle",
        unit: "resource",
        units_per_iteration: 5,
        budget_nanoseconds_per_unit: 2_000.0,
        run: Box::new(move |iterations| {
            let mut checksum = 0_u64;
            for _ in 0..iterations {
                let mut model = Model::default();
                let application = model
                    .create(
                        None,
                        ResourceState::Application(ApplicationState::new("性能".to_owned())),
                    )
                    .unwrap();
                let window = model
                    .create(
                        Some(application),
                        ResourceState::Window(Box::<WindowState>::default()),
                    )
                    .unwrap();
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
                    .unwrap();
                model
                    .create(
                        Some(application),
                        ResourceState::Image {
                            width: 4,
                            height: 4,
                            rgba: vec![0; 64],
                        },
                    )
                    .unwrap();
                model
                    .create(
                        Some(application),
                        ResourceState::Font {
                            family: "性能".to_owned(),
                            bytes: Some(vec![0; 64]),
                        },
                    )
                    .unwrap();
                model.submit_frame(window, vec![0; 256], 1.0).unwrap();
                model.begin_application_run(application).unwrap();
                model.request_application_exit(application).unwrap();
                model.finish_application_run(application, None);
                checksum = checksum
                    .wrapping_add(u64::try_from(model.close(application).unwrap().len()).unwrap());
                black_box(model);
            }
            checksum
        }),
    };

    let render_bytes = render_frame()?;
    let mut engine = RenderEngine::new();
    black_box(engine.render(&render_bytes, 256, 256, 1.0, |_| None)?);
    let render_case = BenchmarkCase {
        name: "cpu_render_256x256",
        unit: "frame",
        units_per_iteration: 1,
        budget_nanoseconds_per_unit: 750_000.0,
        run: Box::new(move |iterations| {
            let mut checksum = 0_u64;
            for _ in 0..iterations {
                let frame = engine
                    .render(black_box(&render_bytes), 256, 256, 1.0, |_| None)
                    .unwrap();
                checksum = checksum.wrapping_add(u64::from(frame.rgba()[0]));
                black_box(frame);
            }
            checksum
        }),
    };

    Ok(vec![protocol_case, event_case, lifecycle_case, render_case])
}

fn protocol_frame() -> Result<Vec<u8>, protocol::ProtocolError> {
    let commands = (0_u16..256)
        .map(|opcode| Command::new(opcode, vec![u8::try_from(opcode % 251).unwrap(); 24]))
        .collect();
    protocol::encode(&Frame {
        commands,
        ..Frame::default()
    })
}

fn render_frame() -> Result<Vec<u8>, protocol::ProtocolError> {
    let mut clear = PayloadWriter::new();
    for value in [12, 12, 12, 255] {
        clear.u8(value);
    }
    let mut commands = vec![Command::new(OP_CLEAR, clear.finish())];
    for index in 0_u32..64 {
        let mut fill = PayloadWriter::new();
        for value in [
            (index % 8) as f32 * 32.0,
            (index / 8) as f32 * 32.0,
            32.0,
            32.0,
        ] {
            fill.f32(value)?;
        }
        for value in [
            u8::try_from(index % 3).unwrap() * 127,
            u8::try_from(index % 5).unwrap() * 63,
            u8::try_from(index % 7).unwrap() * 42,
            255,
        ] {
            fill.u8(value);
        }
        commands.push(Command::new(OP_FILL_RECT, fill.finish()));
    }
    protocol::encode(&Frame {
        commands,
        ..Frame::default()
    })
}

fn measure(mut case: BenchmarkCase) -> BenchmarkResult {
    let iterations = calibrate(&mut case);
    for _ in 0..WARMUP_SAMPLES {
        black_box((case.run)(iterations));
    }
    let mut samples = Vec::with_capacity(MEASURED_SAMPLES);
    for _ in 0..MEASURED_SAMPLES {
        let elapsed = timed_run(&mut case, iterations);
        let units = iterations.saturating_mul(case.units_per_iteration) as f64;
        samples.push(elapsed.as_nanos() as f64 / units);
    }
    let median_value = median(&mut samples);
    let mut deviations = samples
        .iter()
        .map(|sample| (sample - median_value).abs())
        .collect::<Vec<_>>();
    let mad = median(&mut deviations);
    let mad_percent = mad / median_value * 100.0;
    BenchmarkResult {
        name: case.name,
        unit: case.unit,
        iterations_per_sample: iterations,
        median_nanoseconds_per_unit: median_value,
        mad_percent,
        budget_nanoseconds_per_unit: case.budget_nanoseconds_per_unit,
        passed: median_value <= case.budget_nanoseconds_per_unit && mad_percent <= MAX_MAD_PERCENT,
    }
}

fn calibrate(case: &mut BenchmarkCase) -> u64 {
    let mut iterations = 1_u64;
    let elapsed = loop {
        let elapsed = timed_run(case, iterations);
        if elapsed >= CALIBRATION_FLOOR {
            break elapsed;
        }
        iterations = iterations.saturating_mul(2).max(iterations + 1);
    };
    let target = SAMPLE_TARGET.as_nanos();
    let elapsed = elapsed.as_nanos().max(1);
    let scaled = u128::from(iterations)
        .saturating_mul(target)
        .saturating_div(elapsed);
    u64::try_from(scaled).unwrap_or(u64::MAX).max(1)
}

fn timed_run(case: &mut BenchmarkCase, iterations: u64) -> Duration {
    let started = Instant::now();
    black_box((case.run)(black_box(iterations)));
    started.elapsed()
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn report(results: &[BenchmarkResult], passed: bool) -> String {
    let cases = results
        .iter()
        .map(|result| {
            format!(
                concat!(
                    "    {{\n",
                    "      \"name\": \"{}\",\n",
                    "      \"unit\": \"{}\",\n",
                    "      \"iterations_per_sample\": {},\n",
                    "      \"median_nanoseconds_per_unit\": {:.3},\n",
                    "      \"mad_percent\": {:.3},\n",
                    "      \"budget_nanoseconds_per_unit\": {:.3},\n",
                    "      \"passed\": {}\n",
                    "    }}"
                ),
                result.name,
                result.unit,
                result.iterations_per_sample,
                result.median_nanoseconds_per_unit,
                result.mad_percent,
                result.budget_nanoseconds_per_unit,
                result.passed
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        concat!(
            "{{\n",
            "  \"schema\": 1,\n",
            "  \"package_version\": \"{}\",\n",
            "  \"profile\": \"release\",\n",
            "  \"os\": \"{}\",\n",
            "  \"arch\": \"{}\",\n",
            "  \"sample_target_milliseconds\": {},\n",
            "  \"measured_samples\": {},\n",
            "  \"maximum_mad_percent\": {:.3},\n",
            "  \"cases\": [\n",
            "{}\n",
            "  ],\n",
            "  \"passed\": {}\n",
            "}}\n"
        ),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        SAMPLE_TARGET.as_millis(),
        MEASURED_SAMPLES,
        MAX_MAD_PERCENT,
        cases,
        passed
    )
}
