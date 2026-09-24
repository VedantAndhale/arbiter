//! Bounded, local text extraction from modern Office packages. No macros,
//! external links, formulas, or embedded objects are executed.
use anyhow::{Context, Result, ensure};
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

const MAX_XML: u64 = 16 * 1024 * 1024;
const MAX_TEXT: usize = 2 * 1024 * 1024;

pub fn extract(data: &[u8], ext: &str) -> Result<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).context("unreadable Office document")?;
    ensure!(archive.len() <= 4096, "Office document contains too many parts");
    let mut names: Vec<String> = archive
        .file_names()
        .filter(|n| match ext {
            "docx" => *n == "word/document.xml",
            "pptx" => n.starts_with("ppt/slides/slide") && n.ends_with(".xml") && !n.contains("/_rels/"),
            "xlsx" => *n == "xl/sharedStrings.xml" || (n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")),
            _ => false,
        })
        .map(str::to_owned)
        .collect();
    // Natural part order: slide2 before slide10. Shared strings first.
    names.sort_by_key(|n| {
        (
            if n == "xl/sharedStrings.xml" { 0 } else { 1 },
            n.chars().filter(char::is_ascii_digit).collect::<String>().parse::<usize>().unwrap_or(0),
        )
    });
    ensure!(!names.is_empty(), "no supported text parts in Office document");
    let mut total = 0;
    let mut out = String::new();
    let mut shared = Vec::new();
    for name in names {
        let part = archive.by_name(&name)?;
        total += part.size();
        ensure!(total <= MAX_XML, "Office text exceeds 16 MB extraction limit");
        let mut xml = String::new();
        part.take(MAX_XML + 1).read_to_string(&mut xml).context("invalid Office XML")?;
        ensure!(xml.len() as u64 <= MAX_XML, "Office part too large");
        if name == "xl/sharedStrings.xml" {
            shared = shared_strings(&xml)?;
        } else {
            let text = xml_text(&xml, &shared, ext == "xlsx")?;
            out.push_str(&format!("\n--- {name} ---\n{text}\n"));
            ensure!(out.len() <= MAX_TEXT, "Office text exceeds 2 MB output limit");
        }
    }
    ensure!(!out.trim().is_empty(), "document has no extractable text");
    Ok(out)
}

fn decoded(e: &quick_xml::events::BytesText<'_>) -> Result<String> {
    Ok(quick_xml::escape::unescape(e.as_ref())?.into_owned())
}

fn shared_strings(xml: &str) -> Result<Vec<String>> {
    let mut r = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut value = String::new();
    let mut text = false;
    loop {
        match r.read_event()? {
            Event::Start(e) if e.local_name().as_ref() == "si" => value.clear(),
            Event::Start(e) if e.local_name().as_ref() == "t" => text = true,
            Event::Text(e) if text => value.push_str(&decoded(&e)?),
            Event::GeneralRef(e) if text => value.push_str(&quick_xml::escape::unescape(&format!("&{};", e.as_ref()))?),
            Event::End(e) if e.local_name().as_ref() == "t" => text = false,
            Event::End(e) if e.local_name().as_ref() == "si" => out.push(value.clone()),
            Event::DocType(_) => anyhow::bail!("DOCTYPE is not supported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn xml_text(xml: &str, shared: &[String], sheet: bool) -> Result<String> {
    let mut r = Reader::from_str(xml);
    let mut out = String::new();
    let mut capture = false;
    let mut shared_cell = false;
    loop {
        match r.read_event()? {
            Event::Start(e) => match e.local_name().as_ref() {
                "c" if sheet => {
                    shared_cell = e.attributes().flatten().any(|a| a.key.as_ref() == "t" && a.value.as_ref() == "s")
                }
                "t" | "v" => capture = true,
                _ => {}
            },
            Event::Text(e) if capture => {
                let text = decoded(&e)?;
                if sheet && shared_cell {
                    let i = text.trim().parse::<usize>().context("invalid shared string index")?;
                    out.push_str(shared.get(i).context("missing shared string")?);
                } else {
                    out.push_str(&text);
                }
            }
            Event::GeneralRef(e) if capture => {
                out.push_str(&quick_xml::escape::unescape(&format!("&{};", e.as_ref()))?)
            }
            Event::End(e) => match e.local_name().as_ref() {
                "t" | "v" => capture = false,
                "p" | "row" => out.push('\n'),
                "c" if sheet => out.push('\t'),
                _ => {}
            },
            Event::Empty(e) if matches!(e.local_name().as_ref(), "br" | "tab") => out.push(' '),
            Event::DocType(_) => anyhow::bail!("DOCTYPE is not supported"),
            Event::Eof => break,
            _ => {}
        }
        ensure!(out.len() <= MAX_TEXT, "Office text exceeds 2 MB output limit");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn extracts_documents_and_shared_spreadsheet_cells() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"<w:document><w:p><w:r><w:t>Hello &amp; world</w:t></w:r></w:p></w:document>").unwrap();
        let data = zip.finish().unwrap().into_inner();
        assert!(extract(&data, "docx").unwrap().contains("Hello & world\n"));
        let shared = shared_strings("<sst><si><t>Price</t></si></sst>").unwrap();
        assert_eq!(
            xml_text("<row><c t=\"s\"><v>0</v></c><c><v>12</v></c></row>", &shared, true).unwrap(),
            "Price\t12\t\n"
        );
        assert!(xml_text("<!DOCTYPE x><x/>", &[], false).is_err());
    }
}
