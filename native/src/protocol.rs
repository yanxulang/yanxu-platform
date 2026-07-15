//! 言台绘制命令缓冲协议 v1。
//!
//! 所有多字节整数和浮点数使用小端序。每条命令都有独立长度，因此同一主版本的新版
//! 发送方可以增加操作码；旧后端能跳过未知命令。主版本不匹配、截断、非零填充、
//! 非有限浮点和超限数据都会被拒绝。

use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const DRAW_MAGIC: [u8; 4] = *b"YXDR";
pub const DRAW_MAJOR: u16 = 1;
pub const DRAW_MINOR: u16 = 0;
pub const MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_COMMANDS: usize = 65_536;
pub const MAX_COMMAND_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

const FRAME_HEADER_BYTES: usize = 16;
const COMMAND_HEADER_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub opcode: u16,
    pub flags: u16,
    pub payload: Vec<u8>,
}

impl Command {
    #[must_use]
    pub fn new(opcode: u16, payload: Vec<u8>) -> Self {
        Self {
            opcode,
            flags: 0,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub minor: u16,
    pub flags: u32,
    pub commands: Vec<Command>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            minor: DRAW_MINOR,
            flags: 0,
            commands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Limit(&'static str),
    Truncated,
    Magic,
    Major { found: u16 },
    Trailing,
    Padding,
    Utf8,
    NonFinite,
    Value(&'static str),
}

impl Display for ProtocolError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(name) => write!(formatter, "绘制协议数据超过{name}上限"),
            Self::Truncated => formatter.write_str("绘制协议缓冲被截断"),
            Self::Magic => formatter.write_str("绘制协议魔数错误"),
            Self::Major { found } => write!(
                formatter,
                "绘制协议主版本不兼容：后端支持 {DRAW_MAJOR}，收到 {found}"
            ),
            Self::Trailing => formatter.write_str("绘制命令负载包含尾部数据"),
            Self::Padding => formatter.write_str("绘制协议命令填充必须为零"),
            Self::Utf8 => formatter.write_str("绘制协议文字不是 UTF-8"),
            Self::NonFinite => formatter.write_str("绘制协议不接受非有限浮点数"),
            Self::Value(message) => formatter.write_str(message),
        }
    }
}

impl Error for ProtocolError {}

pub fn encode(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    if frame.commands.len() > MAX_COMMANDS {
        return Err(ProtocolError::Limit("命令数量"));
    }
    let mut output = Vec::with_capacity(FRAME_HEADER_BYTES + frame.commands.len() * 16);
    output.extend_from_slice(&DRAW_MAGIC);
    push_u16(&mut output, DRAW_MAJOR);
    push_u16(&mut output, frame.minor);
    push_u32(&mut output, frame.flags);
    push_u32(
        &mut output,
        u32::try_from(frame.commands.len()).map_err(|_| ProtocolError::Limit("命令数量"))?,
    );
    for command in &frame.commands {
        if command.payload.len() > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(ProtocolError::Limit("命令负载"));
        }
        push_u16(&mut output, command.opcode);
        push_u16(&mut output, command.flags);
        push_u32(
            &mut output,
            u32::try_from(command.payload.len()).map_err(|_| ProtocolError::Limit("命令负载"))?,
        );
        output.extend_from_slice(&command.payload);
        while output.len() % 4 != 0 {
            output.push(0);
        }
        if output.len() > MAX_BUFFER_BYTES {
            return Err(ProtocolError::Limit("缓冲"));
        }
    }
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> Result<Frame, ProtocolError> {
    if bytes.len() > MAX_BUFFER_BYTES {
        return Err(ProtocolError::Limit("缓冲"));
    }
    if bytes.len() < FRAME_HEADER_BYTES {
        return Err(ProtocolError::Truncated);
    }
    if bytes[..4] != DRAW_MAGIC {
        return Err(ProtocolError::Magic);
    }
    let major = read_u16(bytes, 4)?;
    if major != DRAW_MAJOR {
        return Err(ProtocolError::Major { found: major });
    }
    let minor = read_u16(bytes, 6)?;
    let flags = read_u32(bytes, 8)?;
    let count =
        usize::try_from(read_u32(bytes, 12)?).map_err(|_| ProtocolError::Limit("命令数量"))?;
    if count > MAX_COMMANDS {
        return Err(ProtocolError::Limit("命令数量"));
    }

    let mut offset = FRAME_HEADER_BYTES;
    let mut commands = Vec::with_capacity(count);
    for _ in 0..count {
        let header_end = offset
            .checked_add(COMMAND_HEADER_BYTES)
            .ok_or(ProtocolError::Truncated)?;
        if header_end > bytes.len() {
            return Err(ProtocolError::Truncated);
        }
        let opcode = read_u16(bytes, offset)?;
        let command_flags = read_u16(bytes, offset + 2)?;
        let payload_length = usize::try_from(read_u32(bytes, offset + 4)?)
            .map_err(|_| ProtocolError::Limit("命令负载"))?;
        if payload_length > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(ProtocolError::Limit("命令负载"));
        }
        let payload_start = header_end;
        let payload_end = payload_start
            .checked_add(payload_length)
            .ok_or(ProtocolError::Truncated)?;
        if payload_end > bytes.len() {
            return Err(ProtocolError::Truncated);
        }
        let aligned_end = align4(payload_end).ok_or(ProtocolError::Truncated)?;
        if aligned_end > bytes.len() {
            return Err(ProtocolError::Truncated);
        }
        if bytes[payload_end..aligned_end]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(ProtocolError::Padding);
        }
        commands.push(Command {
            opcode,
            flags: command_flags,
            payload: bytes[payload_start..payload_end].to_vec(),
        });
        offset = aligned_end;
    }
    if offset != bytes.len() {
        return Err(ProtocolError::Trailing);
    }
    Ok(Frame {
        minor,
        flags,
        commands,
    })
}

#[derive(Debug, Default, Clone)]
pub struct PayloadWriter {
    bytes: Vec<u8>,
}

impl PayloadWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn u16(&mut self, value: u16) {
        push_u16(&mut self.bytes, value);
    }

    pub fn u32(&mut self, value: u32) {
        push_u32(&mut self.bytes, value);
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) -> Result<(), ProtocolError> {
        if !value.is_finite() {
            return Err(ProtocolError::NonFinite);
        }
        self.bytes.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn bytes(&mut self, value: &[u8]) -> Result<(), ProtocolError> {
        self.u32(u32::try_from(value.len()).map_err(|_| ProtocolError::Limit("字节字段"))?);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    pub fn text(&mut self, value: &str) -> Result<(), ProtocolError> {
        if value.len() > MAX_TEXT_BYTES {
            return Err(ProtocolError::Limit("文字"));
        }
        self.bytes(value.as_bytes())
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone)]
pub struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn u8(&mut self) -> Result<u8, ProtocolError> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or(ProtocolError::Truncated)?;
        self.offset += 1;
        Ok(value)
    }

    pub fn u16(&mut self) -> Result<u16, ProtocolError> {
        let value = read_u16(self.bytes, self.offset)?;
        self.offset += 2;
        Ok(value)
    }

    pub fn u32(&mut self) -> Result<u32, ProtocolError> {
        let value = read_u32(self.bytes, self.offset)?;
        self.offset += 4;
        Ok(value)
    }

    pub fn u64(&mut self) -> Result<u64, ProtocolError> {
        let raw = self.take(8)?;
        Ok(u64::from_le_bytes([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ]))
    }

    pub fn f32(&mut self) -> Result<f32, ProtocolError> {
        let value = f32::from_bits(self.u32()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ProtocolError::NonFinite)
        }
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], ProtocolError> {
        let length = usize::try_from(self.u32()?).map_err(|_| ProtocolError::Limit("字节字段"))?;
        if length > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(ProtocolError::Limit("字节字段"));
        }
        self.take(length)
    }

    pub fn text(&mut self) -> Result<&'a str, ProtocolError> {
        let bytes = self.bytes()?;
        if bytes.len() > MAX_TEXT_BYTES {
            return Err(ProtocolError::Limit("文字"));
        }
        std::str::from_utf8(bytes).map_err(|_| ProtocolError::Utf8)
    }

    pub fn finish(self) -> Result<(), ProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ProtocolError::Trailing)
        }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ProtocolError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ProtocolError> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(ProtocolError::Truncated)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ProtocolError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(ProtocolError::Truncated)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|aligned| aligned & !3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frame() -> Frame {
        Frame {
            minor: 4,
            flags: 3,
            commands: vec![
                Command::new(1, vec![10, 20, 30, 255]),
                Command {
                    opcode: 999,
                    flags: 7,
                    payload: vec![1, 2, 3],
                },
            ],
        }
    }

    #[test]
    fn round_trip_preserves_future_minor_and_unknown_commands() {
        let expected = sample_frame();
        let encoded = encode(&expected).unwrap();
        assert_eq!(decode(&encoded).unwrap(), expected);
    }

    #[test]
    fn rejects_incompatible_major() {
        let mut encoded = encode(&sample_frame()).unwrap();
        encoded[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(decode(&encoded), Err(ProtocolError::Major { found: 2 }));
    }

    #[test]
    fn rejects_truncated_and_extra_data() {
        let encoded = encode(&sample_frame()).unwrap();
        assert_eq!(
            decode(&encoded[..encoded.len() - 1]),
            Err(ProtocolError::Truncated)
        );
        let mut trailing = encoded;
        trailing.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(decode(&trailing), Err(ProtocolError::Trailing));
    }

    #[test]
    fn rejects_non_zero_alignment_padding() {
        let mut encoded = encode(&Frame {
            commands: vec![Command::new(1, vec![1])],
            ..Frame::default()
        })
        .unwrap();
        *encoded.last_mut().unwrap() = 9;
        assert_eq!(decode(&encoded), Err(ProtocolError::Padding));
    }

    #[test]
    fn rejects_declared_payload_past_end() {
        let mut encoded = encode(&Frame {
            commands: vec![Command::new(1, Vec::new())],
            ..Frame::default()
        })
        .unwrap();
        encoded[20..24].copy_from_slice(&4_u32.to_le_bytes());
        assert_eq!(decode(&encoded), Err(ProtocolError::Truncated));
    }

    #[test]
    fn payload_helpers_round_trip_native_values() {
        let mut writer = PayloadWriter::new();
        writer.u8(9);
        writer.u16(500);
        writer.u32(80_000);
        writer.u64(9_000_000_000);
        writer.f32(3.5).unwrap();
        writer.text("中文 IME").unwrap();
        let payload = writer.finish();

        let mut reader = PayloadReader::new(&payload);
        assert_eq!(reader.u8().unwrap(), 9);
        assert_eq!(reader.u16().unwrap(), 500);
        assert_eq!(reader.u32().unwrap(), 80_000);
        assert_eq!(reader.u64().unwrap(), 9_000_000_000);
        assert_eq!(reader.f32().unwrap(), 3.5);
        assert_eq!(reader.text().unwrap(), "中文 IME");
        reader.finish().unwrap();
    }

    #[test]
    fn payload_helpers_reject_non_finite_and_invalid_utf8() {
        let mut writer = PayloadWriter::new();
        assert_eq!(writer.f32(f32::NAN), Err(ProtocolError::NonFinite));

        let mut reader = PayloadReader::new(&[1, 0, 0, 0, 0xff]);
        assert_eq!(reader.text(), Err(ProtocolError::Utf8));
    }
}
