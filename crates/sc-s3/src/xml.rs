//! The few S3 XML documents the provider reads: object listings, errors
//! and the start of a multipart upload.

use std::time::SystemTime;

use quick_xml::Reader;
use quick_xml::events::Event;

/// An object in a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// The full key.
    pub key: String,
    /// Size in bytes.
    pub size: u64,
    /// `LastModified`.
    pub modified: Option<SystemTime>,
}

/// One page of `ListObjectsV2`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListPage {
    /// Objects directly under the prefix.
    pub objects: Vec<Object>,
    /// "Folders": common prefixes, ending with the delimiter.
    pub prefixes: Vec<String>,
    /// Token for the next page, when there is one.
    pub next: Option<String>,
}

/// An element's text, with the path of local element names leading to it.
struct Item {
    path: String,
    text: String,
}

/// Flattens a document into `(element path, text)` items, one per
/// element, in document order. Elements without text have empty text.
fn items(xml: &str) -> Result<Vec<Item>, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    // The element whose text is being read; text between elements
    // (indentation) belongs to none.
    let mut open: Option<usize> = None;
    let invalid = |e: &dyn std::fmt::Display| format!("invalid S3 answer: {e}");
    loop {
        match reader.read_event().map_err(|e| invalid(&e))? {
            Event::Start(element) => {
                stack.push(element.local_name().as_ref().to_owned());
                items.push(Item { path: stack.join("/"), text: String::new() });
                open = Some(items.len() - 1);
            }
            Event::Empty(element) => {
                let name = element.local_name().as_ref().to_owned();
                items.push(Item {
                    path: format!("{}/{name}", stack.join("/")),
                    text: String::new(),
                });
                open = None;
            }
            Event::End(_) => {
                stack.pop();
                open = None;
            }
            Event::Text(text) => {
                if let Some(item) = open.map(|i| &mut items[i]) {
                    item.text.push_str(&text.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                let raw = format!("&{};", reference.xml10_content());
                let text = quick_xml::escape::unescape(&raw).map_err(|e| invalid(&e))?;
                if let Some(item) = open.map(|i| &mut items[i]) {
                    item.text.push_str(&text);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(items)
}

/// Parses a `ListBucketResult`.
pub fn parse_list(xml: &str) -> Result<ListPage, String> {
    let mut page = ListPage::default();
    let mut truncated = false;
    for item in items(xml)? {
        let text = item.text;
        match item.path.as_str() {
            "ListBucketResult/Contents" => {
                page.objects.push(Object { key: String::new(), size: 0, modified: None })
            }
            "ListBucketResult/Contents/Key" => last(&mut page.objects).key = text,
            "ListBucketResult/Contents/Size" => {
                last(&mut page.objects).size = text.trim().parse().unwrap_or(0)
            }
            "ListBucketResult/Contents/LastModified" => {
                last(&mut page.objects).modified =
                    text.trim().parse::<jiff::Timestamp>().ok().map(SystemTime::from)
            }
            "ListBucketResult/CommonPrefixes/Prefix" => page.prefixes.push(text),
            "ListBucketResult/IsTruncated" => truncated = text.trim() == "true",
            "ListBucketResult/NextContinuationToken" => page.next = Some(text),
            _ => {}
        }
    }
    if !truncated {
        page.next = None;
    }
    Ok(page)
}

fn last(objects: &mut [Object]) -> &mut Object {
    objects.last_mut().expect("Contents starts every object")
}

/// The `Code` and `Message` of an `Error` document.
pub fn parse_error(xml: &str) -> Option<(String, String)> {
    let items = items(xml).ok()?;
    let find = |path: &str| items.iter().find(|i| i.path == path).map(|i| i.text.clone());
    Some((find("Error/Code")?, find("Error/Message").unwrap_or_default()))
}

/// The `UploadId` of an `InitiateMultipartUploadResult`.
pub fn parse_upload_id(xml: &str) -> Option<String> {
    items(xml).ok()?.into_iter().find(|i| i.path.ends_with("/UploadId")).map(|i| i.text)
}

/// The body of `CompleteMultipartUpload` for the parts' ETags.
pub fn complete_body(etags: &[String]) -> String {
    let mut body = String::from("<CompleteMultipartUpload>");
    for (i, etag) in etags.iter().enumerate() {
        let etag = quick_xml::escape::escape(etag.as_str());
        body.push_str(&format!(
            "<Part><PartNumber>{}</PartNumber><ETag>{etag}</ETag></Part>",
            i + 1
        ));
    }
    body.push_str("</CompleteMultipartUpload>");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    const LIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>bucket</Name><Prefix>photos/</Prefix><KeyCount>3</KeyCount>
  <IsTruncated>true</IsTruncated>
  <NextContinuationToken>1ueGcxLPRx1Tr/XYExHnhbYLgveDs2J/wm36Hy4vbOwM=</NextContinuationToken>
  <Contents>
    <Key>photos/a &amp; b.jpg</Key>
    <LastModified>1970-01-01T00:16:40.000Z</LastModified>
    <ETag>"etag"</ETag><Size>1234</Size><StorageClass>STANDARD</StorageClass>
  </Contents>
  <Contents><Key>photos/c.jpg</Key><Size>0</Size></Contents>
  <CommonPrefixes><Prefix>photos/2026/</Prefix></CommonPrefixes>
</ListBucketResult>"#;

    #[test]
    fn reads_a_listing_page() {
        let page = parse_list(LIST).unwrap();
        assert_eq!(page.objects.len(), 2);
        assert_eq!(page.objects[0].key, "photos/a & b.jpg");
        assert_eq!(page.objects[0].size, 1234);
        assert_eq!(page.objects[0].modified, Some(UNIX_EPOCH + Duration::from_secs(1000)));
        assert_eq!(page.prefixes, ["photos/2026/"]);
        assert!(page.next.as_deref().unwrap().starts_with("1ueG"));
    }

    #[test]
    fn the_last_page_has_no_token() {
        let xml = "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>";
        assert_eq!(parse_list(xml).unwrap(), ListPage::default());
    }

    #[test]
    fn reads_errors_and_upload_ids() {
        let error = "<Error><Code>NoSuchKey</Code><Message>gone</Message></Error>";
        assert_eq!(parse_error(error), Some(("NoSuchKey".into(), "gone".into())));
        assert_eq!(parse_error("not xml <"), None);
        let started = "<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key>\
                       <UploadId>abc</UploadId></InitiateMultipartUploadResult>";
        assert_eq!(parse_upload_id(started).as_deref(), Some("abc"));
    }

    #[test]
    fn builds_the_completion_body() {
        let body = complete_body(&["\"e1\"".into(), "\"e2\"".into()]);
        assert!(body.contains("<PartNumber>2</PartNumber><ETag>&quot;e2&quot;</ETag>"), "{body}");
    }
}
