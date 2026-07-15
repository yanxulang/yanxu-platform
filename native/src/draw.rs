//! 把言序生成的完整结构化绘制树确定性编译为绘制协议 v1。

use crate::data::Data;
use crate::protocol::{self, Command, Frame, PayloadWriter};
use crate::render::{
    OP_CIRCLE, OP_CLEAR, OP_CLIP_RECT, OP_FILL_RECT, OP_GLYPH_RUN, OP_IMAGE, OP_LAYER, OP_LINE,
    OP_PATH, OP_RESTORE, OP_ROUNDED_RECT, OP_SAVE, OP_SHADOW, OP_STROKE_RECT, OP_TEXT,
    OP_TEXT_STYLE, OP_TRANSFORM,
};
use std::collections::BTreeMap;

const MAX_PATH_SEGMENTS: usize = u16::MAX as usize;
const MAX_GLYPHS_PER_RUN: usize = 262_144;

pub fn encode_commands(value: &Data) -> Result<Vec<u8>, &'static str> {
    let commands = array(value)?;
    if commands.len() > protocol::MAX_COMMANDS {
        return Err("PLATFORM_DRAW_LIMIT");
    }
    let commands = commands
        .iter()
        .map(build_command)
        .collect::<Result<Vec<_>, _>>()?;
    protocol::encode(&Frame {
        commands,
        ..Frame::default()
    })
    .map_err(|_| "PLATFORM_DRAW_LIMIT")
}

#[allow(clippy::too_many_lines)]
fn build_command(value: &Data) -> Result<Command, &'static str> {
    let command = map(value)?;
    let kind = required_text(command, "类型")?;
    let mut writer = PayloadWriter::new();
    let opcode = match kind {
        "清空" => {
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_CLEAR
        }
        "保存" => OP_SAVE,
        "恢复" => OP_RESTORE,
        "裁剪矩形" => {
            write_rect(&mut writer, required(command, "矩形")?)?;
            OP_CLIP_RECT
        }
        "变换" => {
            write_float_array(&mut writer, required(command, "矩阵")?, 6)?;
            OP_TRANSFORM
        }
        "图层" => {
            let opacity = required_number(command, "透明度")?;
            if !(0.0..=1.0).contains(&opacity) {
                return Err("PLATFORM_DRAW_RANGE");
            }
            write_f32(&mut writer, opacity)?;
            OP_LAYER
        }
        "填充矩形" => {
            write_rect(&mut writer, required(command, "矩形")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_FILL_RECT
        }
        "描边矩形" => {
            write_rect(&mut writer, required(command, "矩形")?)?;
            write_positive(&mut writer, required_number(command, "宽度")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_STROKE_RECT
        }
        "圆角矩形" => {
            write_rect(&mut writer, required(command, "矩形")?)?;
            write_non_negative(&mut writer, required_number(command, "半径")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_ROUNDED_RECT
        }
        "直线" => {
            write_float_array(&mut writer, required(command, "起点")?, 2)?;
            write_float_array(&mut writer, required(command, "终点")?, 2)?;
            write_positive(&mut writer, required_number(command, "宽度")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_LINE
        }
        "圆形" => {
            write_float_array(&mut writer, required(command, "圆心")?, 2)?;
            write_positive(&mut writer, required_number(command, "半径")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_CIRCLE
        }
        "阴影" => {
            write_rect(&mut writer, required(command, "矩形")?)?;
            write_non_negative(&mut writer, required_number(command, "半径")?)?;
            write_non_negative(&mut writer, required_number(command, "模糊")?)?;
            write_float_array(&mut writer, required(command, "偏移")?, 2)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            OP_SHADOW
        }
        "文字" => {
            write_float_array(&mut writer, required(command, "位置")?, 2)?;
            write_positive(&mut writer, required_number(command, "最大宽")?)?;
            write_positive(&mut writer, required_number(command, "字号")?)?;
            write_positive(&mut writer, required_number(command, "行高")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            let text = required_text(command, "文本")?;
            if command.contains_key("字族")
                || command.contains_key("字重")
                || command.contains_key("斜体")
            {
                write_weight_style(&mut writer, command)?;
                writer
                    .text(optional_text(command, "字族")?.unwrap_or(""))
                    .map_err(|_| "PLATFORM_DRAW_LIMIT")?;
                writer.text(text).map_err(|_| "PLATFORM_DRAW_LIMIT")?;
                OP_TEXT_STYLE
            } else {
                writer.text(text).map_err(|_| "PLATFORM_DRAW_LIMIT")?;
                OP_TEXT
            }
        }
        "字形序列" => {
            write_float_array(&mut writer, required(command, "位置")?, 2)?;
            write_positive(&mut writer, required_number(command, "字号")?)?;
            write_color(&mut writer, required(command, "颜色")?)?;
            write_weight_style(&mut writer, command)?;
            writer
                .text(required_text(command, "字族")?)
                .map_err(|_| "PLATFORM_DRAW_LIMIT")?;
            let glyphs = array(required(command, "字形")?)?;
            if glyphs.len() > MAX_GLYPHS_PER_RUN {
                return Err("PLATFORM_DRAW_LIMIT");
            }
            writer.u32(u32::try_from(glyphs.len()).map_err(|_| "PLATFORM_DRAW_LIMIT")?);
            for glyph in glyphs {
                let glyph = map(glyph)?;
                let glyph_id = u16::try_from(integer(required(glyph, "字形")?)?)
                    .map_err(|_| "PLATFORM_DRAW_RANGE")?;
                writer.u16(glyph_id);
                writer.u16(0);
                write_f32(&mut writer, required_number(glyph, "横坐标")?)?;
                write_f32(&mut writer, required_number(glyph, "基线")?)?;
            }
            OP_GLYPH_RUN
        }
        "图片" => {
            let Data::Resource(handle) = required(command, "图片")? else {
                return Err("PLATFORM_DRAW_TYPE");
            };
            writer.u64(*handle);
            write_rect(&mut writer, required(command, "矩形")?)?;
            let opacity = optional_number(command, "透明度")?.unwrap_or(1.0);
            if !(0.0..=1.0).contains(&opacity) {
                return Err("PLATFORM_DRAW_RANGE");
            }
            write_f32(&mut writer, opacity)?;
            OP_IMAGE
        }
        "路径" => {
            write_path(&mut writer, command)?;
            OP_PATH
        }
        _ => return Err("PLATFORM_DRAW_COMMAND"),
    };
    Ok(Command::new(opcode, writer.finish()))
}

fn write_path(
    writer: &mut PayloadWriter,
    command: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    let style = match required_text(command, "样式")? {
        "填充" => 0,
        "描边" => 1,
        _ => return Err("PLATFORM_DRAW_VALUE"),
    };
    writer.u8(style);
    writer.u8(u8::from(
        optional_bool(command, "奇偶填充")?.unwrap_or(false),
    ));
    let segments = array(required(command, "分段")?)?;
    if segments.len() > MAX_PATH_SEGMENTS {
        return Err("PLATFORM_DRAW_LIMIT");
    }
    writer.u16(u16::try_from(segments.len()).map_err(|_| "PLATFORM_DRAW_LIMIT")?);
    let stroke_width = optional_number(command, "宽度")?.unwrap_or(1.0);
    if style == 1 && stroke_width <= 0.0 {
        return Err("PLATFORM_DRAW_RANGE");
    }
    write_f32(writer, stroke_width)?;
    write_color(writer, required(command, "颜色")?)?;
    for segment in segments {
        let segment = map(segment)?;
        match required_text(segment, "类型")? {
            "移动" => {
                writer.u8(1);
                write_float_array(writer, required(segment, "点")?, 2)?;
            }
            "直线" => {
                writer.u8(2);
                write_float_array(writer, required(segment, "点")?, 2)?;
            }
            "二次曲线" => {
                writer.u8(3);
                write_float_array(writer, required(segment, "控制点")?, 2)?;
                write_float_array(writer, required(segment, "终点")?, 2)?;
            }
            "三次曲线" => {
                writer.u8(4);
                write_float_array(writer, required(segment, "控制点一")?, 2)?;
                write_float_array(writer, required(segment, "控制点二")?, 2)?;
                write_float_array(writer, required(segment, "终点")?, 2)?;
            }
            "闭合" => writer.u8(5),
            _ => return Err("PLATFORM_DRAW_VALUE"),
        }
    }
    Ok(())
}

fn write_rect(writer: &mut PayloadWriter, value: &Data) -> Result<(), &'static str> {
    let values = number_array(value, 4)?;
    if values[2] <= 0.0 || values[3] <= 0.0 {
        return Err("PLATFORM_DRAW_RANGE");
    }
    for value in values {
        write_f32(writer, value)?;
    }
    Ok(())
}

fn write_float_array(
    writer: &mut PayloadWriter,
    value: &Data,
    expected: usize,
) -> Result<(), &'static str> {
    for value in number_array(value, expected)? {
        write_f32(writer, value)?;
    }
    Ok(())
}

fn write_color(writer: &mut PayloadWriter, value: &Data) -> Result<(), &'static str> {
    let values = array(value)?;
    if !(values.len() == 3 || values.len() == 4) {
        return Err("PLATFORM_DRAW_TYPE");
    }
    for value in values {
        let value = integer(value)?;
        writer.u8(u8::try_from(value).map_err(|_| "PLATFORM_DRAW_RANGE")?);
    }
    if values.len() == 3 {
        writer.u8(255);
    }
    Ok(())
}

fn write_weight_style(
    writer: &mut PayloadWriter,
    command: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    let weight = optional_integer(command, "字重")?.unwrap_or(400);
    if !(1..=1_000).contains(&weight) {
        return Err("PLATFORM_DRAW_RANGE");
    }
    writer.u16(u16::try_from(weight).map_err(|_| "PLATFORM_DRAW_RANGE")?);
    writer.u8(u8::from(optional_bool(command, "斜体")?.unwrap_or(false)));
    writer.u8(0);
    Ok(())
}

fn write_positive(writer: &mut PayloadWriter, value: f64) -> Result<(), &'static str> {
    if value <= 0.0 {
        return Err("PLATFORM_DRAW_RANGE");
    }
    write_f32(writer, value)
}

fn write_non_negative(writer: &mut PayloadWriter, value: f64) -> Result<(), &'static str> {
    if value < 0.0 {
        return Err("PLATFORM_DRAW_RANGE");
    }
    write_f32(writer, value)
}

fn write_f32(writer: &mut PayloadWriter, value: f64) -> Result<(), &'static str> {
    let value = value as f32;
    if !value.is_finite() {
        return Err("PLATFORM_DRAW_NUMBER");
    }
    writer.f32(value).map_err(|_| "PLATFORM_DRAW_NUMBER")
}

fn number_array(value: &Data, expected: usize) -> Result<Vec<f64>, &'static str> {
    let values = array(value)?;
    if values.len() != expected {
        return Err("PLATFORM_DRAW_TYPE");
    }
    values.iter().map(number).collect()
}

fn required<'a>(map: &'a BTreeMap<String, Data>, key: &str) -> Result<&'a Data, &'static str> {
    map.get(key).ok_or("PLATFORM_DRAW_FIELD")
}

fn required_text<'a>(map: &'a BTreeMap<String, Data>, key: &str) -> Result<&'a str, &'static str> {
    text(required(map, key)?)
}

fn required_number(map: &BTreeMap<String, Data>, key: &str) -> Result<f64, &'static str> {
    number(required(map, key)?)
}

fn optional_number(map: &BTreeMap<String, Data>, key: &str) -> Result<Option<f64>, &'static str> {
    map.get(key).map(number).transpose()
}

fn optional_integer(map: &BTreeMap<String, Data>, key: &str) -> Result<Option<i64>, &'static str> {
    map.get(key).map(integer).transpose()
}

fn optional_text<'a>(
    map: &'a BTreeMap<String, Data>,
    key: &str,
) -> Result<Option<&'a str>, &'static str> {
    map.get(key).map(text).transpose()
}

fn optional_bool(map: &BTreeMap<String, Data>, key: &str) -> Result<Option<bool>, &'static str> {
    map.get(key).map(boolean).transpose()
}

fn array(value: &Data) -> Result<&[Data], &'static str> {
    let Data::Array(value) = value else {
        return Err("PLATFORM_DRAW_TYPE");
    };
    Ok(value)
}

fn map(value: &Data) -> Result<&BTreeMap<String, Data>, &'static str> {
    let Data::Map(value) = value else {
        return Err("PLATFORM_DRAW_TYPE");
    };
    Ok(value)
}

fn text(value: &Data) -> Result<&str, &'static str> {
    let Data::String(value) = value else {
        return Err("PLATFORM_DRAW_TYPE");
    };
    Ok(value)
}

fn number(value: &Data) -> Result<f64, &'static str> {
    value
        .as_number()
        .filter(|value| value.is_finite())
        .ok_or("PLATFORM_DRAW_TYPE")
}

fn integer(value: &Data) -> Result<i64, &'static str> {
    let Data::Integer(value) = value else {
        return Err("PLATFORM_DRAW_TYPE");
    };
    Ok(*value)
}

fn boolean(value: &Data) -> Result<bool, &'static str> {
    let Data::Bool(value) = value else {
        return Err("PLATFORM_DRAW_TYPE");
    };
    Ok(*value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderEngine;

    #[test]
    fn compiles_complete_structured_frame_to_binary_protocol() {
        let commands = Data::Array(vec![
            Data::map([
                ("类型", Data::String("清空".to_owned())),
                (
                    "颜色",
                    Data::Array(vec![
                        Data::Integer(255),
                        Data::Integer(255),
                        Data::Integer(255),
                        Data::Integer(255),
                    ]),
                ),
            ]),
            Data::map([
                ("类型", Data::String("圆角矩形".to_owned())),
                (
                    "矩形",
                    Data::Array(vec![
                        Data::Integer(1),
                        Data::Integer(1),
                        Data::Integer(6),
                        Data::Integer(6),
                    ]),
                ),
                ("半径", Data::Integer(2)),
                (
                    "颜色",
                    Data::Array(vec![
                        Data::Integer(20),
                        Data::Integer(100),
                        Data::Integer(220),
                        Data::Integer(255),
                    ]),
                ),
            ]),
        ]);
        let encoded = encode_commands(&commands).unwrap();
        let frame = protocol::decode(&encoded).unwrap();
        assert_eq!(frame.commands.len(), 2);
        let rendered = RenderEngine::new()
            .render(&encoded, 8, 8, 1.0, |_| None)
            .unwrap();
        assert_eq!(
            &rendered.rgba()[4 * (4 * 8 + 4)..][..4],
            &[20, 100, 220, 255]
        );
    }

    #[test]
    fn compiles_styled_text_and_glyph_runs_as_v1_1_commands() {
        let color = || {
            Data::Array(vec![
                Data::Integer(10),
                Data::Integer(20),
                Data::Integer(30),
                Data::Integer(255),
            ])
        };
        let commands = Data::Array(vec![
            Data::map([
                ("类型", Data::String("文字".to_owned())),
                (
                    "位置",
                    Data::Array(vec![Data::Integer(1), Data::Integer(2)]),
                ),
                ("最大宽", Data::Integer(200)),
                ("字号", Data::Integer(16)),
                ("行高", Data::Integer(22)),
                ("颜色", color()),
                ("字族", Data::String("测试字族".to_owned())),
                ("字重", Data::Integer(700)),
                ("斜体", Data::Bool(true)),
                ("文本", Data::String("言序".to_owned())),
            ]),
            Data::map([
                ("类型", Data::String("字形序列".to_owned())),
                (
                    "位置",
                    Data::Array(vec![Data::Integer(3), Data::Integer(4)]),
                ),
                ("字号", Data::Integer(16)),
                ("颜色", color()),
                ("字族", Data::String("测试字族".to_owned())),
                ("字重", Data::Integer(400)),
                (
                    "字形",
                    Data::Array(vec![Data::map([
                        ("字形", Data::Integer(42)),
                        ("横坐标", Data::Integer(0)),
                        ("基线", Data::Integer(18)),
                    ])]),
                ),
            ]),
        ]);
        let frame = protocol::decode(&encode_commands(&commands).unwrap()).unwrap();
        assert_eq!(frame.minor, 1);
        assert_eq!(
            frame
                .commands
                .iter()
                .map(|command| command.opcode)
                .collect::<Vec<_>>(),
            vec![OP_TEXT_STYLE, OP_GLYPH_RUN]
        );
    }

    #[test]
    fn rejects_unknown_commands_bad_ranges_and_missing_fields() {
        assert_eq!(
            encode_commands(&Data::Array(vec![Data::map([(
                "类型",
                Data::String("未来命令".to_owned()),
            )])])),
            Err("PLATFORM_DRAW_COMMAND")
        );
        assert_eq!(
            encode_commands(&Data::Array(vec![Data::map([
                ("类型", Data::String("图层".to_owned())),
                ("透明度", Data::Number(2.0)),
            ])])),
            Err("PLATFORM_DRAW_RANGE")
        );
        assert_eq!(
            encode_commands(&Data::Array(vec![Data::map([(
                "类型",
                Data::String("文字".to_owned()),
            )])])),
            Err("PLATFORM_DRAW_FIELD")
        );
    }
}
