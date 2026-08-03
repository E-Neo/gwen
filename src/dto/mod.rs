use serde::{Deserialize, Serialize};

pub mod xml;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ColorType {
    Rgb,
    Scheme,
    Hsl,
    Scrgb,
    System,
    Preset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillType {
    Solid,
    Gradient,
    Pattern,
    Picture,
    NoFill,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillDto {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub fill_type: Option<FillType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFormatDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineCap {
    Rnd,
    Sq,
    Flat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompoundLine {
    Sng,
    Dbl,
    ThickThin,
    ThinThick,
    Tri,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineDash {
    Solid,
    Dot,
    Dash,
    LgDash,
    DashDot,
    LgDashDot,
    LgDashDotDot,
    SysDash,
    SysDot,
    SysDashDot,
    SysDashDotDot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<LineCap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compound: Option<CompoundLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<LineDash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<FillDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperlinkDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDto {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<FontDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<HyperlinkDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MsoAutoSize {
    None,
    ShapeToFitText,
    TextToFitShape,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerticalAnchor {
    Top,
    Middle,
    Bottom,
    Justified,
    Distributed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Default level-0 paragraph style from `a:lstStyle/a:lvl1pPr`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_paragraph_style: Option<ParagraphDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GridColDto {
    pub width: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableCellDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_span: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_span: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h_merge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v_merge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame: Option<TextFrameDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableRowDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    pub cells: Vec<TableCellDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableDto {
    pub grid: Vec<GridColDto>,
    pub rows: Vec<TableRowDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChartSeriesDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChartDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<ChartSeriesDto>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CropDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
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

    // Group child coordinate system (a:chOff / a:chExt), for group shapes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ch_off_x: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ch_off_y: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ch_ext_cx: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ch_ext_cy: Option<i64>,

    pub is_placeholder: bool,
    pub has_text_frame: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<FillDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineDto>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_format: Option<PlaceholderFormatDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_shape_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame: Option<TextFrameDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<CropDto>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TableDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart: Option<ChartDto>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub shapes: Option<Vec<ShapeDto>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SlideDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub shapes: Vec<ShapeDto>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeTypeInput {
    Textbox,
    Picture,
    Table,
    Chart,
    AutoShape,
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
    pub auto_shape_type: Option<String>,
    /// For picture shapes: set by add.rs after storing the image; used as r:embed
    pub image_r_id: Option<String>,
    /// For chart shapes: inline chart data for creating a new chart part
    pub chart: Option<ChartDto>,
    /// For chart shapes: set by add.rs after creating the chart part; used as r:id in c:chart
    pub chart_r_id: Option<String>,
    /// For chart shapes: relationship ID to an existing chart part
    pub r_id: Option<String>,
    /// For table shapes: table definition (grid, rows, cells)
    pub table: Option<TableDto>,
    /// Fill for auto/text shapes
    pub fill: Option<FillDto>,
    /// Outline for shapes
    pub outline: Option<OutlineDto>,
}
