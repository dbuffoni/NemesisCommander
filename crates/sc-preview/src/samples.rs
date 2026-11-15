//! Small sample files of every previewed kind, generated in code.
//!
//! They serve the tests of every interface, the demo folder of
//! `scripts/demo-tree.sh` and the screenshots of the manual, so nobody has
//! to keep binary files in the repository. `cargo run -p sc-preview
//! --example samples -- DIR` writes them all to `DIR`.

use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::io::Write as _;

/// Every sample: file name and contents.
pub fn all() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("guide.md", markdown().as_bytes().to_vec()),
        ("offer.docx", docx()),
        ("budget.xlsx", xlsx()),
        ("kickoff.pptx", pptx()),
        ("report.pdf", pdf(&REPORT)),
        ("torus.stl", torus_stl(48, 24)),
    ]
}

/// A zip file of `members` (name, contents), as Office files are.
pub fn zip(members: &[(&str, &str)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, contents) in members {
        out.start_file(*name, zip::write::SimpleFileOptions::default()).expect("writing to memory");
        out.write_all(contents.as_bytes()).expect("writing to memory");
    }
    out.finish().expect("writing to memory").into_inner()
}

/// A Markdown guide using most of the syntax.
pub fn markdown() -> &'static str {
    "# Project notes

SuperCommander shows **Markdown** as formatted text, with *emphasis*,
`inline code` and [links](https://example.org).

## Checklist

- Install the AppImage
- Connect the server
  - with SSH keys
  - or with a one-time token
- Copy the photos

## Commands

```sh
sc copy ~/photos sftp://nas/backup/
sc hash --algorithm blake3 report.pdf
```

> Tip: press F3 on any file to view it, Ctrl+Q for the quick view.

| Key | Action |
|-----|--------|
| F5  | Copy   |
| F6  | Move   |
| F8  | Delete |

---

That is all.
"
}

/// A Word document: heading, paragraphs with bold text, a list, a table.
pub fn docx() -> Vec<u8> {
    let paragraph = |style: &str, runs: &str| {
        let style = if style.is_empty() {
            String::new()
        } else {
            format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#)
        };
        format!("<w:p>{style}{runs}</w:p>")
    };
    let run = |text: &str| format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#);
    let bold = |text: &str| {
        format!(r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#)
    };
    let item = |text: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>{}</w:p>"#,
            run(text)
        )
    };
    let cell = |text: &str| format!("<w:tc><w:p>{}</w:p></w:tc>", run(text));
    let row = |cells: &[&str]| {
        format!("<w:tr>{}</w:tr>", cells.iter().map(|c| cell(c)).collect::<String>())
    };
    let body = [
        paragraph("Title", &run("Offer for the office move")),
        paragraph(
            "",
            &(run("Dear customer, here is our offer. The total is ")
                + &bold("4,350 EUR")
                + &run(", VAT included.")),
        ),
        paragraph("Heading1", &run("What is included")),
        item("Packing of 40 boxes"),
        item("Transport on 12 March"),
        item("Assembly of the desks"),
        paragraph("Heading1", &run("Costs")),
        format!(
            "<w:tbl>{}{}{}{}</w:tbl>",
            row(&["Service", "Hours", "Price"]),
            row(&["Packing", "6", "900"]),
            row(&["Transport", "8", "2,400"]),
            row(&["Assembly", "5", "1,050"])
        ),
        paragraph("", &run("Kind regards, the moving team.")),
    ]
    .concat();
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_DOCX),
        ("_rels/.rels", &root_rels("word/document.xml")),
        ("word/document.xml", &document),
    ])
}

/// A spreadsheet with one sheet of numbers.
pub fn xlsx() -> Vec<u8> {
    let rows = [
        ("Item", "Q1", "Q2"),
        ("Rent", "3600", "3600"),
        ("Servers", "1250.5", "1310"),
        ("Travel", "820", "455.25"),
        ("Training", "0", "1200"),
    ];
    let mut sheet = String::new();
    for (index, (name, q1, q2)) in rows.iter().enumerate() {
        let r = index + 1;
        let value = |column: char, text: &str| {
            if text.parse::<f64>().is_ok() {
                format!(r#"<c r="{column}{r}"><v>{text}</v></c>"#)
            } else {
                format!(r#"<c r="{column}{r}" t="inlineStr"><is><t>{text}</t></is></c>"#)
            }
        };
        let _ = write!(
            sheet,
            r#"<row r="{r}">{}{}{}</row>"#,
            value('A', name),
            value('B', q1),
            value('C', q2)
        );
    }
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet}</sheetData></worksheet>"#
    );
    let workbook = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget 2027" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    zip(&[
        ("[Content_Types].xml", types),
        ("_rels/.rels", &root_rels("xl/workbook.xml")),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", &sheet),
    ])
}

/// A presentation of three slides.
pub fn pptx() -> Vec<u8> {
    let slide = |lines: &[&str]| {
        let paragraphs: String =
            lines.iter().map(|line| format!("<a:p><a:r><a:t>{line}</a:t></a:r></a:p>")).collect();
        format!(
            r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    };
    let one = slide(&["Project kickoff", "12 March 2027"]);
    let two = slide(&["Goals", "Move every server by April", "No downtime for customers"]);
    let three = slide(&["Next steps", "Inventory of the racks", "Book the transport"]);
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_PPTX),
        ("_rels/.rels", &root_rels("ppt/presentation.xml")),
        ("ppt/slides/slide1.xml", &one),
        ("ppt/slides/slide2.xml", &two),
        ("ppt/slides/slide3.xml", &three),
    ])
}

/// The pages of [`all`]'s PDF: title and lines of text.
const REPORT: [(&str, &[&str]); 3] = [
    (
        "Quarterly report",
        &[
            "Revenue grew 12% over the last quarter.",
            "Costs stayed flat.",
            "",
            "See the next pages for details.",
        ],
    ),
    (
        "Servers",
        &["All 14 servers were moved to the new rack.", "Average load: 38%.", "Disk use: 61%."],
    ),
    ("Plans", &["Replace the backup disks in May.", "Add a second agent route for the office."]),
];

/// A PDF with one A5 page per entry of `pages` (title, lines), each with a
/// coloured band, its title and its lines in Helvetica.
pub fn pdf(pages: &[(&str, &[&str])]) -> Vec<u8> {
    const COLOURS: [&str; 3] = ["0.16 0.38 0.71", "0.13 0.55 0.33", "0.80 0.40 0.10"];
    // Objects: 1 catalog, 2 pages, 3 font, then a page and its contents per page.
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut kids = Vec::new();
    for (index, (title, lines)) in pages.iter().enumerate() {
        let page_object = objects.len() + 1;
        kids.push(format!("{page_object} 0 R"));
        let mut stream = format!(
            "{} rg 0 520 420 75 re f\n1 1 1 rg BT /F1 26 Tf 30 545 Td ({}) Tj ET\n0.15 0.15 0.15 rg BT /F1 13 Tf 30 480 Td 20 TL\n",
            COLOURS[index % COLOURS.len()],
            escape(title)
        );
        for line in lines.iter() {
            let _ = writeln!(stream, "({}) Tj T*", escape(line));
        }
        let _ =
            write!(stream, "ET\n0.5 0.5 0.5 rg BT /F1 10 Tf 360 30 Td (Page {}) Tj ET", index + 1);
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 420 595] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            page_object + 1
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
    }
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len());

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    let size = objects.len() + 1;
    out.extend(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").bytes(),
    );
    out
}

/// Text for a PDF string: backslashes and parentheses escaped.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")
}

/// A torus (a ring) as an ASCII STL file, `around` × `across` segments.
pub fn torus_stl(around: usize, across: usize) -> Vec<u8> {
    let (ring, tube) = (2.0f32, 0.7f32);
    let point = |i: usize, j: usize| {
        let (u, v) = (i as f32 / around as f32 * TAU, j as f32 / across as f32 * TAU);
        let r = ring + tube * v.cos();
        [r * u.cos(), r * u.sin(), tube * v.sin()]
    };
    let mut out = String::from("solid torus\n");
    for i in 0..around {
        for j in 0..across {
            let (a, b, c, d) = (point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1));
            for triangle in [[a, b, c], [a, c, d]] {
                out.push_str(" facet normal 0 0 0\n  outer loop\n");
                for [x, y, z] in triangle {
                    let _ = writeln!(out, "   vertex {x:.5} {y:.5} {z:.5}");
                }
                out.push_str("  endloop\n endfacet\n");
            }
        }
    }
    out.push_str("endsolid torus\n");
    out.into_bytes()
}

fn root_rels(target: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{target}"/></Relationships>"#
    )
}

const CONTENT_TYPES_DOCX: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

const CONTENT_TYPES_PPTX: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/slides/slide3.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#;
