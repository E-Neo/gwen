use serde::{Deserialize, Serialize};

pub mod xml;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShapeType {
    AutoShape,
    Picture,
    Placeholder,
    TextBox,
    Line,
    Group,
    Chart,
    Table,
    Media,
    Freeform,
    EmbeddedOleObject,
    LinkedOleObject,
    Comment,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlaceholderType {
    Title,
    Body,
    CenterTitle,
    SubTitle,
    Object,
    Chart,
    Table,
    ClipArt,
    Picture,
    Diagram,
    Media,
    SlideImage,
    SlideNumber,
    Footer,
    Header,
    DateTime,
    VerticalObject,
    VerticalTitle,
    VerticalBody,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaceholderFormatDto {
    pub idx: i32,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ph_type: Option<PlaceholderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sz: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ColorType {
    Rgb,
    Scheme,
    Hsl,
    Scrgb,
    System,
    Preset,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ColorFormatDto {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub color_type: Option<ColorType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FontDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFormatDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HyperlinkDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunDto {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<FontDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<HyperlinkDto>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
    Distribute,
    ThaiDistribute,
    JustifiedLow,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MsoAutoSize {
    None,
    ShapeToFitText,
    TextToFitShape,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerticalAnchor {
    Top,
    Middle,
    Bottom,
    Justified,
    Distributed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParagraphDto {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<RunDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<FontDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextFrameDto {
    pub paragraphs: Vec<ParagraphDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_size: Option<MsoAutoSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_wrap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_anchor: Option<VerticalAnchor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_right: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_top: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_bottom: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShapeDto {
    pub shape_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "shape_type")]
    pub shape_type: ShapeType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,

    pub is_placeholder: bool,
    pub has_text_frame: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_format: Option<PlaceholderFormatDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_shape_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame: Option<TextFrameDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub shapes: Option<Vec<ShapeDto>>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SlideDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub shapes: Vec<ShapeDto>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeTypeInput {
    Textbox,
    Picture,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddShape {
    #[serde(rename = "type")]
    pub shape_type: ShapeTypeInput,
    pub left: Option<i64>,
    pub top: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub text: Option<String>,
    pub image: Option<String>,
    pub shape_id: Option<u32>,
}
