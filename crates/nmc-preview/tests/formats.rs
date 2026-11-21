//! Every format from real (small, generated) files.

use std::io::Write;
use std::sync::Arc;

use nmc_preview::model::{Camera, Mesh, render};
use nmc_preview::office::{self, Format};
use nmc_preview::rich::{BlockKind, layout, line_text};
use nmc_preview::{Kind, pdf};

/// A zip file with `members` (name, contents).
fn zip(members: &[(&str, &str)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, contents) in members {
        out.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(contents.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn text_of(blocks: &[nmc_preview::rich::Block]) -> Vec<String> {
    layout(blocks, 80).iter().map(line_text).collect()
}

#[test]
fn word_documents_keep_headings_lists_bold_and_tables() {
    let document = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Offer &amp; terms</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Price is </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>1,200 EUR</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>first point</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Item</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cost</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>Desk</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>300</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;
    let data = zip(&[("word/document.xml", document)]);
    let blocks = office::read(Format::Docx, &data).unwrap();
    assert!(matches!(blocks[0].kind, BlockKind::Heading(1)));
    assert_eq!(blocks[0].text(), "Offer & terms");
    assert!(blocks[1].spans.iter().any(|s| s.text == "1,200 EUR" && s.style.bold));
    assert!(matches!(blocks[2].kind, BlockKind::Item { .. }));
    let text = text_of(&blocks);
    assert!(text.contains(&"Item │ Cost".to_owned()), "{text:?}");
    assert!(text.contains(&"Desk │ 300".to_owned()), "{text:?}");
}

#[test]
fn open_document_texts_and_presentations() {
    let content = r#"<?xml version="1.0"?>
<office:document-content xmlns:office="o" xmlns:text="t" xmlns:table="tb" xmlns:draw="d"><office:body><office:text>
<text:h text:outline-level="2">Minutes</text:h>
<text:p>Present:<text:s text:c="2"/>Ada</text:p>
<text:list><text:list-item><text:p>budget approved</text:p></text:list-item></text:list>
<table:table><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell/><table:table-cell><text:p>c</text:p></table:table-cell></table:table-row></table:table>
</office:text></office:body></office:document-content>"#;
    let blocks = office::read(Format::Odt, &zip(&[("content.xml", content)])).unwrap();
    assert!(matches!(blocks[0].kind, BlockKind::Heading(2)));
    assert_eq!(blocks[1].text(), "Present:  Ada");
    assert!(matches!(&blocks[2].kind, BlockKind::Item { depth: 0, .. }));
    assert!(matches!(&blocks[3].kind, BlockKind::Table { rows, .. } if rows[0] == ["a", "", "c"]));

    let slides = r#"<office:document-content xmlns:office="o" xmlns:text="t" xmlns:draw="d"><office:body><office:presentation>
<draw:page draw:name="Intro"><draw:frame><draw:text-box><text:p>Welcome</text:p></draw:text-box></draw:frame></draw:page>
<draw:page draw:name="Plan"><draw:frame><draw:text-box><text:p>Step one</text:p></draw:text-box></draw:frame></draw:page>
</office:presentation></office:body></office:document-content>"#;
    let text = text_of(&office::read(Format::Odp, &zip(&[("content.xml", slides)])).unwrap());
    assert!(
        text.contains(&"Slide 1: Intro".to_owned()) && text.contains(&"Step one".to_owned()),
        "{text:?}"
    );
}

#[test]
fn powerpoint_slides_in_order() {
    let slide = |text: &str| {
        format!(
            r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    };
    let (two, ten, one) = (slide("Second"), slide("Tenth"), slide("First"));
    let data = zip(&[
        ("ppt/slides/slide2.xml", &two),
        ("ppt/slides/slide10.xml", &ten),
        ("ppt/slides/slide1.xml", &one),
    ]);
    let text: Vec<String> = text_of(&office::read(Format::Pptx, &data).unwrap())
        .into_iter()
        .filter(|l| !l.is_empty() && !l.starts_with('─'))
        .collect();
    assert_eq!(text, ["Slide 1", "First", "Slide 2", "Second", "Slide 10", "Tenth"]);
}

#[test]
fn spreadsheets_become_tables() {
    let workbook = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Room</t></is></c><c r="B1" t="inlineStr"><is><t>Litres</t></is></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>Kitchen</t></is></c><c r="B2"><v>5</v></c></row>
<row r="3"><c r="A3" t="inlineStr"><is><t>Hall</t></is></c><c r="B3"><v>2.5</v></c></row>
</sheetData></worksheet>"#;
    let types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let data = zip(&[
        ("[Content_Types].xml", types),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", sheet),
    ]);
    let text = text_of(&office::read(Format::Sheet, &data).unwrap());
    assert_eq!(text[0], "Budget");
    assert!(text.contains(&"Kitchen │ 5".to_owned()), "{text:?}");
    assert!(text.contains(&"Hall    │ 2.5".to_owned()), "{text:?}");
    assert!(office::read(Format::Sheet, b"not a workbook").is_err());
}

/// A one-page PDF (200 × 100 points) with a red square at the left.
pub fn tiny_pdf() -> Vec<u8> {
    let stream = "1 0 0 rg 10 10 80 80 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
    ];
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
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn pdf_pages_are_drawn() {
    let data = Arc::new(tiny_pdf());
    assert_eq!(pdf::page_count(&data).unwrap(), 1);
    let page = pdf::render_page(&data, 0, 400, 400).unwrap();
    assert_eq!((page.width(), page.height()), (400, 200), "fits, keeping the page's shape");
    let red = page.get_pixel(100, 100).0;
    assert!(red[0] > 200 && red[1] < 60, "the square: {red:?}");
    let white = page.get_pixel(300, 100).0;
    assert!(white.iter().all(|c| *c > 240), "the paper: {white:?}");
    assert!(pdf::render_page(&data, 3, 100, 100).is_err());
    assert!(pdf::page_count(&Arc::new(b"%PDF-broken".to_vec())).is_err());
}

const CUBE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
f 1 2 3 4\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

#[test]
fn models_load_in_every_format() {
    let obj = Mesh::load("cube.obj", CUBE_OBJ.as_bytes()).unwrap();
    assert_eq!(obj.triangle_count(), 12);
    assert_eq!(obj.bounds(), ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));

    let stl = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
    assert_eq!(Mesh::load("t.STL", stl.as_bytes()).unwrap().triangle_count(), 1);

    let ply = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
    assert_eq!(
        Mesh::load("q.ply", ply.as_bytes()).unwrap().triangle_count(),
        2,
        "a quad is two triangles"
    );

    // Binary PLY, little endian: one triangle.
    let mut binary = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        binary.extend(value.to_le_bytes());
    }
    binary.push(3);
    for index in [0i32, 1, 2] {
        binary.extend(index.to_le_bytes());
    }
    assert_eq!(Mesh::load("b.ply", &binary).unwrap().triangle_count(), 1);

    // glTF with its buffer inline: one triangle, moved by its node.
    let buffer: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let gltf = format!(
        r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"translation":[10,0,0]}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
        "bufferViews":[{{"buffer":0,"byteLength":36}}],
        "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
        base64(&buffer)
    );
    let mesh = Mesh::load("t.gltf", gltf.as_bytes()).unwrap();
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(mesh.bounds().0[0], 10.0, "the node's translation applies");

    assert!(Mesh::load("x.obj", b"").is_err());
    assert!(Mesh::load("x.stl", b"garbage").is_err());
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn models_are_drawn_shaded_and_centred() {
    let cube = Mesh::load("cube.obj", CUBE_OBJ.as_bytes()).unwrap();
    let picture = render(&cube, Camera::default(), 200, 100);
    assert_eq!((picture.width(), picture.height()), (200, 100));
    let centre = picture.get_pixel(100, 50).0;
    let corner = picture.get_pixel(2, 2).0;
    assert_ne!(centre, corner, "the model covers the centre, not the corner");
    assert!(centre[0] > centre[2], "the model is drawn in a warm colour");
    // Turning the camera changes the picture.
    let turned = render(&cube, Camera { yaw: 80.0, ..Camera::default() }, 200, 100);
    assert_ne!(picture, turned);
}

#[test]
fn kinds_of_real_names() {
    assert_eq!(Kind::of("Q3 report.xlsx").map(Kind::label), Some("Spreadsheet"));
    assert_eq!(Kind::of("deck.pptx").map(Kind::label), Some("PowerPoint slides (text)"));
}

#[test]
fn every_sample_previews() {
    for (name, data) in nmc_preview::samples::all() {
        match Kind::of(name).unwrap() {
            Kind::Markdown => assert!(
                !nmc_preview::markdown::parse(std::str::from_utf8(&data).unwrap()).is_empty()
            ),
            Kind::Office(format) => {
                let text = text_of(&office::read(format, &data).unwrap()).join("\n");
                assert!(text.len() > 40, "{name}: {text}");
            }
            Kind::Pdf => {
                let data = Arc::new(data);
                assert_eq!(pdf::page_count(&data).unwrap(), 3);
                let page = pdf::render_page(&data, 0, 420, 595).unwrap();
                // The title is white text on the blue band.
                let band_row = (page.height() as f32 * 0.07) as u32;
                let white = (0..page.width())
                    .filter(|&x| page.get_pixel(x, band_row).0[..3] == [255, 255, 255])
                    .count();
                let blue = (0..page.width())
                    .filter(|&x| page.get_pixel(x, 5).0[2] > 150 && page.get_pixel(x, 5).0[0] < 80)
                    .count();
                assert!(blue > page.width() as usize / 2, "band drawn");
                assert!(white > 0, "title drawn");
            }
            Kind::Model => {
                assert_eq!(Mesh::load(name, &data).unwrap().triangle_count(), 48 * 24 * 2)
            }
            Kind::Ebook(format) => {
                let text = text_of(&nmc_preview::ebook::read(format, &data).unwrap()).join("\n");
                assert!(
                    text.contains("A short story") && text.contains("Everything is on the page"),
                    "{text}"
                );
            }
            Kind::Audio => {
                let blocks = nmc_preview::media::audio(name, &data, true).unwrap();
                assert!(text_of(&blocks).join("\n").contains("8000 Hz"));
            }
            Kind::Video => unreachable!("no video sample"),
        }
    }
}
