use crate::abi;
use crate::accessibility::{
    ACCESSIBILITY_MAJOR, ACCESSIBILITY_MINOR, MAX_SEMANTIC_ACTIONS, MAX_SEMANTIC_CHILDREN,
    MAX_SEMANTIC_COORDINATE, MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_NODE_TEXT_BYTES, MAX_SEMANTIC_NODES,
    MAX_SEMANTIC_TEXT_BYTES, SEMANTIC_ACTIONS, SEMANTIC_ROLES, SEMANTIC_STATES,
};
use crate::backend::{self, PLATFORM_MAJOR, PLATFORM_MINOR};
use crate::data::Data;
use crate::draw::{
    DRAW_OPERATIONS, MAX_GLYPHS_PER_RUN as MAX_ENCODED_GLYPHS, MAX_PATH_SEGMENTS, PATH_OPERATIONS,
};
use crate::event::{
    DEFAULT_EVENT_CAPACITY, EVENT_KINDS, EVENT_MAJOR, EVENT_MINOR, EventBatcher, EventKind,
    PlatformEvent,
};
use crate::protocol;
use crate::render::{
    MAX_DIMENSION, MAX_GLYPHS_PER_RUN as MAX_RENDERED_GLYPHS, MAX_PATH_VERBS, MAX_STATE_DEPTH,
};
use std::collections::BTreeSet;
use std::fmt::Write as _;

fn write_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", u32::from(character)).unwrap();
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn write_string_list(output: &mut String, values: &[&str], indent: &str) {
    output.push_str("[\n");
    for (index, value) in values.iter().enumerate() {
        output.push_str(indent);
        write_json_string(output, value);
        output.push_str(if index + 1 == values.len() {
            "\n"
        } else {
            ",\n"
        });
    }
    output.push_str(&indent[..indent.len() - 2]);
    output.push(']');
}

const fn data_kind(value: &Data) -> &'static str {
    match value {
        Data::Nil => "null",
        Data::Bool(_) => "bool",
        Data::Integer(_) => "integer",
        Data::Number(_) => "number",
        Data::String(_) => "string",
        Data::Bytes(_) => "bytes",
        Data::Array(_) => "array",
        Data::Map(_) => "map",
        Data::Resource(_) => "resource",
        Data::Callback(_) => "callback",
    }
}

fn event_envelope() -> (Vec<String>, Vec<String>) {
    let mut batcher = EventBatcher::with_capacity(1);
    batcher
        .push(PlatformEvent::new(EventKind::Timer, None, 0.0))
        .unwrap();
    let batch = batcher.take_data().unwrap();
    let batch = batch.as_map().unwrap();
    let envelope = batch.keys().cloned().collect();
    let Data::Array(events) = &batch["事件"] else {
        panic!("事件批次必须包含事件列");
    };
    let event = events[0].as_map().unwrap();
    let common = event.keys().cloned().collect();
    (envelope, common)
}

fn protocol_snapshot() -> String {
    assert_eq!(MAX_PATH_SEGMENTS, MAX_PATH_VERBS);
    assert_eq!(MAX_ENCODED_GLYPHS, MAX_RENDERED_GLYPHS);

    let event_names = EVENT_KINDS
        .iter()
        .map(|kind| kind.name())
        .collect::<BTreeSet<_>>();
    assert_eq!(event_names.len(), EVENT_KINDS.len());

    for values in [SEMANTIC_ROLES, SEMANTIC_STATES, SEMANTIC_ACTIONS] {
        assert_eq!(
            values.iter().copied().collect::<BTreeSet<_>>().len(),
            values.len()
        );
    }

    for (index, (opcode, _, since_minor)) in DRAW_OPERATIONS.iter().enumerate() {
        assert_eq!(usize::from(*opcode), index + 1);
        assert!(*since_minor <= protocol::DRAW_MINOR);
    }
    for (index, (opcode, _)) in PATH_OPERATIONS.iter().enumerate() {
        assert_eq!(usize::from(*opcode), index + 1);
    }

    let protocol_info = backend::protocol_info();
    let protocol_info = protocol_info.as_map().unwrap();
    let capabilities = backend::capabilities();
    let capabilities = capabilities.as_map().unwrap();
    let resource_limits = capabilities["应用资源硬上限"].as_map().unwrap();
    let (event_envelope, event_common) = event_envelope();

    let mut output = String::new();
    output.push_str("{\n");
    output.push_str("  \"schema\": 1,\n");
    output.push_str("  \"contract\": \"yanxu-platform-protocols-1.0\",\n");
    output.push_str("  \"platform\": {\n");
    writeln!(
        output,
        "    \"version\": {{ \"major\": {PLATFORM_MAJOR}, \"minor\": {PLATFORM_MINOR} }},"
    )
    .unwrap();
    writeln!(output, "    \"native_abi\": {},", abi::ABI).unwrap();
    output.push_str("    \"handshake_fields\": [\n");
    for (index, (name, value)) in protocol_info.iter().enumerate() {
        let Data::Integer(value) = value else {
            panic!("协议查询字段必须是整数");
        };
        output.push_str("      { \"name\": ");
        write_json_string(&mut output, name);
        writeln!(
            output,
            ", \"value\": {value} }}{}",
            if index + 1 == protocol_info.len() {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    output.push_str("    ],\n");
    output.push_str("    \"capability_fields\": [\n");
    for (index, (name, value)) in capabilities.iter().enumerate() {
        output.push_str("      { \"name\": ");
        write_json_string(&mut output, name);
        output.push_str(", \"type\": ");
        let kind = if name == "原生无障碍后端" {
            assert!(matches!(value, Data::String(_) | Data::Nil));
            "string-or-null"
        } else {
            data_kind(value)
        };
        write_json_string(&mut output, kind);
        output.push_str(if index + 1 == capabilities.len() {
            " }\n"
        } else {
            " },\n"
        });
    }
    output.push_str("    ],\n");
    output.push_str("    \"resource_limit_fields\": [\n");
    for (index, (name, value)) in resource_limits.iter().enumerate() {
        let Data::Integer(value) = value else {
            panic!("资源硬上限必须是整数");
        };
        output.push_str("      { \"name\": ");
        write_json_string(&mut output, name);
        output.push_str(", \"type\": \"integer\", \"value\": ");
        write!(output, "{value}").unwrap();
        output.push_str(if index + 1 == resource_limits.len() {
            " }\n"
        } else {
            " },\n"
        });
    }
    output.push_str("    ]\n");
    output.push_str("  },\n");

    output.push_str("  \"event\": {\n");
    writeln!(
        output,
        "    \"version\": {{ \"major\": {EVENT_MAJOR}, \"minor\": {EVENT_MINOR} }},"
    )
    .unwrap();
    writeln!(
        output,
        "    \"default_queue_capacity\": {DEFAULT_EVENT_CAPACITY},"
    )
    .unwrap();
    output.push_str("    \"envelope_fields\": ");
    let event_envelope = event_envelope
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    write_string_list(&mut output, &event_envelope, "      ");
    output.push_str(",\n");
    output.push_str("    \"common_fields\": ");
    let event_common = event_common.iter().map(String::as_str).collect::<Vec<_>>();
    write_string_list(&mut output, &event_common, "      ");
    output.push_str(",\n");
    output.push_str("    \"events\": [\n");
    for (index, kind) in EVENT_KINDS.iter().enumerate() {
        output.push_str("      { \"name\": ");
        write_json_string(&mut output, kind.name());
        output.push_str(", \"coalescing\": ");
        write_json_string(&mut output, kind.coalescing_name());
        output.push_str(if index + 1 == EVENT_KINDS.len() {
            " }\n"
        } else {
            " },\n"
        });
    }
    output.push_str("    ]\n");
    output.push_str("  },\n");

    output.push_str("  \"accessibility\": {\n");
    writeln!(
        output,
        "    \"version\": {{ \"major\": {ACCESSIBILITY_MAJOR}, \"minor\": {ACCESSIBILITY_MINOR} }},"
    )
    .unwrap();
    output.push_str("    \"roles\": ");
    write_string_list(&mut output, SEMANTIC_ROLES, "      ");
    output.push_str(",\n");
    output.push_str("    \"states\": ");
    write_string_list(&mut output, SEMANTIC_STATES, "      ");
    output.push_str(",\n");
    output.push_str("    \"actions\": ");
    write_string_list(&mut output, SEMANTIC_ACTIONS, "      ");
    output.push_str(",\n");
    output.push_str("    \"limits\": {\n");
    writeln!(output, "      \"nodes\": {MAX_SEMANTIC_NODES},").unwrap();
    writeln!(output, "      \"depth\": {MAX_SEMANTIC_DEPTH},").unwrap();
    writeln!(
        output,
        "      \"children_per_node\": {MAX_SEMANTIC_CHILDREN},"
    )
    .unwrap();
    writeln!(
        output,
        "      \"actions_per_node\": {MAX_SEMANTIC_ACTIONS},"
    )
    .unwrap();
    writeln!(
        output,
        "      \"node_text_bytes\": {MAX_SEMANTIC_NODE_TEXT_BYTES},"
    )
    .unwrap();
    writeln!(
        output,
        "      \"total_text_bytes\": {MAX_SEMANTIC_TEXT_BYTES},"
    )
    .unwrap();
    writeln!(
        output,
        "      \"coordinate_absolute\": {MAX_SEMANTIC_COORDINATE}"
    )
    .unwrap();
    output.push_str("    }\n");
    output.push_str("  },\n");

    output.push_str("  \"draw\": {\n");
    let magic = std::str::from_utf8(&protocol::DRAW_MAGIC).unwrap();
    output.push_str("    \"magic\": ");
    write_json_string(&mut output, magic);
    output.push_str(",\n");
    writeln!(
        output,
        "    \"version\": {{ \"major\": {}, \"minor\": {} }},",
        protocol::DRAW_MAJOR,
        protocol::DRAW_MINOR
    )
    .unwrap();
    output.push_str("    \"byte_order\": \"little\",\n");
    writeln!(
        output,
        "    \"frame_header_bytes\": {},",
        protocol::FRAME_HEADER_BYTES
    )
    .unwrap();
    writeln!(
        output,
        "    \"command_header_bytes\": {},",
        protocol::COMMAND_HEADER_BYTES
    )
    .unwrap();
    writeln!(
        output,
        "    \"command_alignment_bytes\": {},",
        protocol::COMMAND_ALIGNMENT_BYTES
    )
    .unwrap();
    output.push_str("    \"operations\": [\n");
    for (index, (opcode, name, since_minor)) in DRAW_OPERATIONS.iter().enumerate() {
        write!(output, "      {{ \"opcode\": {opcode}, \"name\": ").unwrap();
        write_json_string(&mut output, name);
        writeln!(
            output,
            ", \"since_minor\": {since_minor} }}{}",
            if index + 1 == DRAW_OPERATIONS.len() {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    output.push_str("    ],\n");
    output.push_str("    \"path_operations\": [\n");
    for (index, (opcode, name)) in PATH_OPERATIONS.iter().enumerate() {
        write!(output, "      {{ \"opcode\": {opcode}, \"name\": ").unwrap();
        write_json_string(&mut output, name);
        output.push_str(if index + 1 == PATH_OPERATIONS.len() {
            " }\n"
        } else {
            " },\n"
        });
    }
    output.push_str("    ],\n");
    output.push_str("    \"limits\": {\n");
    writeln!(
        output,
        "      \"buffer_bytes\": {},",
        protocol::MAX_BUFFER_BYTES
    )
    .unwrap();
    writeln!(output, "      \"commands\": {},", protocol::MAX_COMMANDS).unwrap();
    writeln!(
        output,
        "      \"command_payload_bytes\": {},",
        protocol::MAX_COMMAND_PAYLOAD_BYTES
    )
    .unwrap();
    writeln!(
        output,
        "      \"text_bytes\": {},",
        protocol::MAX_TEXT_BYTES
    )
    .unwrap();
    writeln!(output, "      \"path_segments\": {MAX_PATH_SEGMENTS},").unwrap();
    writeln!(output, "      \"glyphs_per_run\": {MAX_ENCODED_GLYPHS},").unwrap();
    writeln!(output, "      \"state_depth\": {MAX_STATE_DEPTH},").unwrap();
    writeln!(output, "      \"surface_dimension\": {MAX_DIMENSION},").unwrap();
    writeln!(output, "      \"image_dimension\": {MAX_DIMENSION}").unwrap();
    output.push_str("    }\n");
    output.push_str("  }\n");
    output.push_str("}\n");
    output
}

#[test]
fn protocols_match_the_1_0_frozen_snapshot() {
    assert_eq!(
        protocol_snapshot(),
        include_str!("../../api/protocol-contract-v1.json")
    );
}
