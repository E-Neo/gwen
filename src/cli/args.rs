use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "pptx-engineer",
    about = "Lossless query and modification of PowerPoint (.pptx) files",
    after_help = concat!(
        "\u{1b}[1;4mPath syntax:\u{1b}[0m\n",
        r#"  All paths address the presentation as a JSON tree. Indices and field names
  mirror the JSON emitted by `query`, so drill down interactively:

    p                                      whole presentation
    p.slides[N]                            Nth slide
    p.slides[N].shapes[M]                  Nth shape on a slide
    p.slides[N].shapes[M].text_frame       text frame of a shape
    p.slides[N].shapes[M].text             plain text of a shape
    p.slides[N].shapes[M].fill             fill of a shape
    p.slides[N].shapes[M].outline          outline of a shape
    p.slides[N].shapes[M].chart            chart data of a chart shape
    p.slides[N].shapes[M].table            table of a table shape
    p.slides[N].shapes[M].crop             picture crop of a picture shape
    p.slides[N].notes                      notes slide (null when absent)
    p.slides[N].notes.shapes[M]            shape on the notes slide
    p.slides[N].slide_layout               reference {master, layout, name}
    p.slide_masters[N]                     Nth slide master
    p.slide_masters[N].slide_layouts[M]    Mth layout of master N
    p.theme.colors.<name>                  theme color slot (dk1, lt1, dk2, lt2,
                                           accent1..6, hlink, folHlink)
    p.theme.fonts.major                    theme major font
    p.theme.fonts.minor                    theme minor font
    p.core_properties.<name>               document property (title, creator, ...)

"#,
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer query deck.pptx --path 'p.slides'
  pptx-engineer query deck.pptx --path 'p.slides[0].shapes[0].text_frame'"#
    ),
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Query a path and print its JSON view
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer query deck.pptx --path 'p.slides'
  pptx-engineer query deck.pptx --path 'p.slides[0].shapes[0].text_frame'
  pptx-engineer query deck.pptx --path 'p.slides[0].shapes[1].table'
  pptx-engineer query deck.pptx --path 'p.slides[0].shapes[2].chart'
  pptx-engineer query deck.pptx --path 'p.slides[0].slide_layout'
  pptx-engineer query deck.pptx --path 'p.slide_masters[0].slide_layouts[0].name'
  pptx-engineer query deck.pptx --path 'p.theme.colors'

Output is a single compact JSON document on stdout. Pass --media <DIR> to
also write every image referenced by the target slide into <DIR>."#
    ))]
    Query {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// Directory to extract media files into
        #[arg(long)]
        media: Option<String>,
    },
    /// Insert a new element (shape, paragraph, chart series, or table row/column)
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer add deck.pptx --path 'p.slides[0].shapes' --value '{"type":"textbox","left":100000,"top":100000,"width":5000000,"height":500000,"text":"Hi"}' --output out.pptx
  pptx-engineer add deck.pptx --path 'p.slides[0].shapes[0].text_frame.paragraphs' --value '{"runs":[{"text":"new para"}]}' --output out.pptx
  pptx-engineer add deck.pptx --path 'p.slides[0].shapes[2].chart.series' --value '{"name":"S2","categories":["A","B"],"values":[1,2]}' --output out.pptx
  pptx-engineer add deck.pptx --path 'p.slides[0].shapes[1].table.rows' --value '{"height":370840,"cells":[{"text_frame":{"paragraphs":[{"runs":[{"text":"New"}]}]}},{}]}' --output out.pptx
  pptx-engineer add deck.pptx --path 'p.slides[0].shapes[1].table.grid' --value '{"width":2000000}' --output out.pptx"#
    ))]
    Add {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// JSON value describing the new element
        #[arg(long)]
        value: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Delete the target element (slide, shape, paragraph, series, or table row)
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer remove deck.pptx --path 'p.slides[0]' --output out.pptx
  pptx-engineer remove deck.pptx --path 'p.slides[0].shapes[0]' --output out.pptx
  pptx-engineer remove deck.pptx --path 'p.slides[0].shapes[0].text_frame.paragraphs[0]' --output out.pptx
  pptx-engineer remove deck.pptx --path 'p.slides[0].shapes[2].chart.series[0]' --output out.pptx
  pptx-engineer remove deck.pptx --path 'p.slides[0].shapes[1].table.rows[0]' --output out.pptx"#
    ))]
    Remove {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Replace a property, element, or whole subtree
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer replace deck.pptx --path 'p.slides[0].shapes[0].text' --value '"Hello World"' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.slides[0].shapes[0].text_frame' --value '{"paragraphs":[{"runs":[{"text":"Hi","font":{"size":2000,"bold":true}}]}]}' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.slides[0].shapes[0].fill' --value '{"type":"solid","color":{"theme_color":"accent1"}}' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.slides[0].shapes[0].outline' --value '{"width":9525,"fill":{"type":"solid","color":{"theme_color":"accent1"}}}' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.slides[0].shapes[0].left' --value '100000' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.theme.colors.accent1' --value '"00FF00"' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.core_properties.title' --value '"New Title"' --output out.pptx
  pptx-engineer replace deck.pptx --path 'p.slides[0].background.fill.color' --value '"C7000B"' --output out.pptx"#
    ))]
    Replace {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// New value
        #[arg(long)]
        value: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Move a subtree (shape or text element) within the package
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer move deck.pptx --from 'p.slides[0].shapes[0]' --to 'p.slides[0].shapes[1]' --output out.pptx
  pptx-engineer move deck.pptx --from 'p.slides[0].shapes[0].text_frame.paragraphs[0]' --to 'p.slides[0].shapes[0].text_frame.paragraphs' --output out.pptx"#
    ))]
    Move {
        /// Input PPTX file path
        input: String,

        /// Source path
        #[arg(long)]
        from: String,

        /// Destination path
        #[arg(long)]
        to: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Copy a subtree (shape or text element)
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer copy deck.pptx --from 'p.slides[0].shapes[0]' --to 'p.slides[0].shapes[1]' --output out.pptx
  pptx-engineer copy deck.pptx --from 'p.slides[0].shapes[0].text_frame.paragraphs[0]' --to 'p.slides[0].shapes[0].text_frame.paragraphs' --output out.pptx"#
    ))]
    Copy {
        /// Input PPTX file path
        input: String,

        /// Source path
        #[arg(long)]
        from: String,

        /// Destination path
        #[arg(long)]
        to: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Create a new empty presentation from the built-in template
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer new deck.pptx
  pptx-engineer new deck.pptx --size 4:3"#
    ))]
    New {
        /// Output PPTX file path
        output: String,

        /// Slide size: '16:9' or '4:3'
        #[arg(long, default_value = "16:9")]
        size: String,
    },
}
