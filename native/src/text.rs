//! 字体匹配、复杂文字整形、测量和原文索引命中测试。

use cosmic_text::fontdb::{Family as DbFamily, Query, Stretch, Style, Weight};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_LAYOUT_WIDTH: f32 = 1_000_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct TextOptions {
    pub family: Option<String>,
    pub weight: u16,
    pub italic: bool,
    pub font_size: f32,
    pub line_height: f32,
    pub max_width: Option<f32>,
    pub wrap: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            family: None,
            weight: 400,
            italic: false,
            font_size: 16.0,
            line_height: 22.0,
            max_width: None,
            wrap: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontMatch {
    pub family: String,
    pub postscript_name: String,
    pub weight: u16,
    pub italic: bool,
    pub monospaced: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapedGlyph {
    pub font: String,
    pub glyph_id: u16,
    pub weight: u16,
    pub italic: bool,
    pub source_start: usize,
    pub source_end: usize,
    pub x: f32,
    pub baseline: f32,
    pub width: f32,
    pub rtl: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub source_line: usize,
    pub top: f32,
    pub baseline: f32,
    pub height: f32,
    pub width: f32,
    pub rtl: bool,
    pub glyph_start: usize,
    pub glyph_end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub glyphs: Vec<ShapedGlyph>,
    pub lines: Vec<TextLine>,
}

impl TextLayout {
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32, text_bytes: usize) -> usize {
        let Some(line) = self
            .lines
            .iter()
            .find(|line| y >= line.top && y < line.top + line.height)
            .or_else(|| {
                if y < 0.0 {
                    self.lines.first()
                } else {
                    self.lines.last()
                }
            })
        else {
            return 0;
        };
        let glyphs = &self.glyphs[line.glyph_start..line.glyph_end];
        let Some(first) = glyphs.first() else {
            return text_bytes;
        };
        if x <= first.x {
            return if line.rtl {
                first.source_end
            } else {
                first.source_start
            };
        }
        for glyph in glyphs {
            if x < glyph.x + glyph.width * 0.5 {
                return if glyph.rtl {
                    glyph.source_end
                } else {
                    glyph.source_start
                };
            }
            if x < glyph.x + glyph.width {
                return if glyph.rtl {
                    glyph.source_start
                } else {
                    glyph.source_end
                };
            }
        }
        glyphs.last().map_or(text_bytes, |glyph| {
            if glyph.rtl {
                glyph.source_start
            } else {
                glyph.source_end
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    Limit(&'static str),
    Options,
    Font,
}

impl Display for TextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(name) => write!(formatter, "文字服务数据超过{name}上限"),
            Self::Options => formatter.write_str("文字尺寸、行高或宽度无效"),
            Self::Font => formatter.write_str("字体文件无效或没有可用字族"),
        }
    }
}

impl Error for TextError {}

pub struct TextService {
    font_system: FontSystem,
}

impl Default for TextService {
    fn default() -> Self {
        Self::new()
    }
}

impl TextService {
    #[must_use]
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
        }
    }

    #[must_use]
    pub fn families(&self) -> Vec<String> {
        self.font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[must_use]
    pub fn match_font(&self, family: &str, weight: u16, italic: bool) -> Option<FontMatch> {
        let families = if family.is_empty() {
            vec![DbFamily::SansSerif]
        } else {
            vec![DbFamily::Name(family)]
        };
        let id = self.font_system.db().query(&Query {
            families: &families,
            weight: Weight(weight.clamp(1, 1_000)),
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        })?;
        let face = self.font_system.db().face(id)?;
        Some(FontMatch {
            family: face
                .families
                .first()
                .map_or_else(|| family.to_owned(), |(name, _)| name.clone()),
            postscript_name: face.post_script_name.clone(),
            weight: face.weight.0,
            italic: face.style != Style::Normal,
            monospaced: face.monospaced,
        })
    }

    pub fn load_font(&mut self, bytes: Vec<u8>) -> Result<Vec<String>, TextError> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err(TextError::Limit("字体"));
        }
        let before: BTreeSet<_> = self.font_system.db().faces().map(|face| face.id).collect();
        self.font_system.db_mut().load_font_data(bytes);
        let families: BTreeSet<_> = self
            .font_system
            .db()
            .faces()
            .filter(|face| !before.contains(&face.id))
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        if families.is_empty() {
            Err(TextError::Font)
        } else {
            Ok(families.into_iter().collect())
        }
    }

    pub fn shape(&mut self, text: &str, options: &TextOptions) -> Result<TextLayout, TextError> {
        validate(text, options)?;
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(options.font_size, options.line_height),
        );
        buffer.set_wrap(if options.wrap {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        });
        buffer.set_size(options.max_width, None);
        let attrs = options
            .family
            .as_deref()
            .filter(|family| !family.is_empty())
            .map_or_else(Attrs::new, |family| {
                Attrs::new().family(Family::Name(family))
            })
            .weight(Weight(options.weight))
            .style(if options.italic {
                Style::Italic
            } else {
                Style::Normal
            });
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, true);

        let line_offsets = line_offsets(text);
        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;
        let mut baseline: Option<f32> = None;
        let mut glyphs = Vec::new();
        let mut lines = Vec::new();
        for run in buffer.layout_runs() {
            let source_offset = line_offsets.get(run.line_i).copied().unwrap_or(0);
            let glyph_start = glyphs.len();
            for glyph in run.glyphs {
                let (family, italic) = self.font_system.db().face(glyph.font_id).map_or_else(
                    || (options.family.clone().unwrap_or_default(), options.italic),
                    |face| {
                        (
                            face.families
                                .first()
                                .map_or_else(String::new, |(name, _)| name.clone()),
                            options.italic || face.style != Style::Normal,
                        )
                    },
                );
                glyphs.push(ShapedGlyph {
                    font: family,
                    glyph_id: glyph.glyph_id,
                    source_start: source_offset.saturating_add(glyph.start),
                    source_end: source_offset.saturating_add(glyph.end),
                    x: glyph.x,
                    baseline: run.line_y + glyph.y,
                    width: glyph.w,
                    rtl: run.rtl,
                    weight: glyph.font_weight.0,
                    italic,
                });
            }
            let glyph_end = glyphs.len();
            width = width.max(run.line_w);
            height = height.max(run.line_top + run.line_height);
            baseline.get_or_insert(run.line_y);
            lines.push(TextLine {
                source_line: run.line_i,
                top: run.line_top,
                baseline: run.line_y,
                height: run.line_height,
                width: run.line_w,
                rtl: run.rtl,
                glyph_start,
                glyph_end,
            });
        }
        Ok(TextLayout {
            width,
            height,
            baseline: baseline.unwrap_or(options.font_size),
            glyphs,
            lines,
        })
    }
}

fn validate(text: &str, options: &TextOptions) -> Result<(), TextError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(TextError::Limit("文字"));
    }
    if !options.font_size.is_finite()
        || !options.line_height.is_finite()
        || options.font_size <= 0.0
        || options.line_height <= 0.0
        || options.font_size > MAX_LAYOUT_WIDTH
        || options.line_height > MAX_LAYOUT_WIDTH
        || !(1..=1_000).contains(&options.weight)
        || options
            .max_width
            .is_some_and(|width| !width.is_finite() || width <= 0.0 || width > MAX_LAYOUT_WIDTH)
    {
        return Err(TextError::Options);
    }
    Ok(())
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_unicode_with_original_utf8_indices_and_metrics() {
        let mut service = TextService::new();
        let layout = service
            .shape(
                "A中B",
                &TextOptions {
                    max_width: Some(200.0),
                    ..TextOptions::default()
                },
            )
            .unwrap();
        assert!(layout.width > 0.0);
        assert!(layout.height > 0.0);
        assert!(layout.baseline > 0.0);
        assert!(!layout.glyphs.is_empty());
        assert!(
            layout
                .glyphs
                .iter()
                .all(|glyph| glyph.source_start <= glyph.source_end && glyph.source_end <= 5)
        );
        assert!(layout.glyphs.iter().any(|glyph| glyph.source_end >= 4));
        assert!(layout.glyphs.iter().all(|glyph| !glyph.font.is_empty()));
        assert!(
            layout
                .glyphs
                .iter()
                .all(|glyph| (1..=1_000).contains(&glyph.weight))
        );
    }

    #[test]
    fn hit_testing_returns_utf8_boundaries() {
        let mut service = TextService::new();
        let layout = service.shape("言序", &TextOptions::default()).unwrap();
        for x in [0.0, layout.width * 0.5, layout.width + 10.0] {
            let index = layout.hit_test(x, 1.0, "言序".len());
            assert!("言序".is_char_boundary(index));
        }
    }

    #[test]
    fn rejects_invalid_sizes_and_font_data() {
        let mut service = TextService::new();
        assert_eq!(
            service.shape(
                "text",
                &TextOptions {
                    font_size: f32::NAN,
                    ..TextOptions::default()
                }
            ),
            Err(TextError::Options)
        );
        assert_eq!(
            service.shape(
                "text",
                &TextOptions {
                    weight: 0,
                    ..TextOptions::default()
                }
            ),
            Err(TextError::Options)
        );
        assert_eq!(service.load_font(vec![1, 2, 3]), Err(TextError::Font));
    }

    #[test]
    fn enumerates_and_matches_an_available_system_family() {
        let service = TextService::new();
        let families = service.families();
        if let Some(family) = families.first() {
            let matched = service.match_font(family, 400, false).unwrap();
            assert!(!matched.family.is_empty());
            assert!(!matched.postscript_name.is_empty());
        }
    }
}
