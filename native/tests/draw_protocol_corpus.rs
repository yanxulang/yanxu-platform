use yanxu_platform_native::protocol::{Command, Frame, ProtocolError, decode, encode};

const MALFORMED_CORPUS: &str = include_str!("corpus/draw-v1-malformed.tsv");

fn error_kind(error: &ProtocolError) -> &'static str {
    match error {
        ProtocolError::Limit(_) => "limit",
        ProtocolError::Truncated => "truncated",
        ProtocolError::Magic => "magic",
        ProtocolError::Major { .. } => "major",
        ProtocolError::Trailing => "trailing",
        ProtocolError::Padding => "padding",
        ProtocolError::Utf8 => "utf8",
        ProtocolError::NonFinite => "non-finite",
        ProtocolError::Value(_) => "value",
    }
}

fn decode_hex(input: &str) -> Vec<u8> {
    if input == "-" {
        return Vec::new();
    }
    assert!(input.len().is_multiple_of(2), "十六进制语料长度必须为偶数");
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
        .collect()
}

fn nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("十六进制语料包含非法字符"),
    }
}

#[test]
fn rejects_versioned_malformed_draw_corpus_with_stable_categories() {
    let mut cases = 0_usize;
    for (line_index, raw_line) in MALFORMED_CORPUS.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            3,
            "语料第 {} 行必须包含三个字段",
            line_index + 1
        );
        let name = fields[0];
        let expected = fields[1];
        let bytes = decode_hex(fields[2]);
        let error = decode(&bytes).unwrap_err();
        assert_eq!(error_kind(&error), expected, "语料用例 {name}");
        cases += 1;
    }
    assert_eq!(cases, 16, "语料用例数量变化时必须显式更新门禁");
}

#[test]
fn every_strict_prefix_of_a_valid_frame_is_rejected() {
    let encoded = encode(&Frame {
        minor: u16::MAX,
        flags: u32::MAX,
        commands: vec![Command {
            opcode: u16::MAX,
            flags: u16::MAX,
            payload: vec![1, 2, 3],
        }],
    })
    .unwrap();

    for end in 0..encoded.len() {
        assert!(
            decode(&encoded[..end]).is_err(),
            "严格前缀 {end} 不得被接受为完整帧"
        );
    }
    assert!(decode(&encoded).is_ok());
}

#[test]
fn single_byte_boundary_mutations_never_escape_bounded_results() {
    let seed = encode(&Frame {
        commands: vec![
            Command::new(1, vec![0, 1, 2, 3]),
            Command::new(999, vec![4, 5, 6]),
        ],
        ..Frame::default()
    })
    .unwrap();

    let mut mutations = 0_usize;
    for index in 0..seed.len() {
        for replacement in [0_u8, 0xff] {
            if seed[index] == replacement {
                continue;
            }
            let mut bytes = seed.clone();
            bytes[index] = replacement;
            if let Ok(frame) = decode(&bytes) {
                let normalized = encode(&frame).unwrap();
                assert_eq!(decode(&normalized).unwrap(), frame);
            }
            mutations += 1;
        }
    }
    assert!(mutations >= seed.len());
}
