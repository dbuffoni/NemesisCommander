//! Reading `PROPFIND` answers (`207 Multi-Status`).
//!
//! Only the few properties a file manager needs are read. Element names are
//! matched by their local name: every server puts them in the `DAV:`
//! namespace, but prefixes differ (`D:`, `d:`, `lp1:`, none).

use std::time::SystemTime;

use quick_xml::Reader;
use quick_xml::events::Event;

/// One `<response>` of a multi-status answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resource {
    /// The `href`, percent-decoded, as sent (a path or a full URL).
    pub href: String,
    /// `resourcetype` contains `collection`.
    pub is_collection: bool,
    /// `getcontentlength`.
    pub length: Option<u64>,
    /// `getlastmodified`.
    pub modified: Option<SystemTime>,
}

/// The body of a `PROPFIND` request for those properties.
pub const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop>
<D:resourcetype/><D:getcontentlength/><D:getlastmodified/>
</D:prop></D:propfind>"#;

/// Which text element we are inside.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Href,
    Length,
    Modified,
    Status,
}

/// Parses a multi-status document. Properties inside a `propstat` whose
/// status is not 2xx are ignored (servers list unknown properties there).
pub fn parse_multistatus(xml: &str) -> Result<Vec<Resource>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut resources = Vec::new();
    let mut current: Option<Resource> = None;
    // The properties of the propstat being read, kept until its status.
    let mut pending = Resource::default();
    let mut status_ok = true;
    let mut field = Field::None;
    let mut in_resourcetype = false;
    // Text of the current element, which may arrive in several pieces.
    let mut buffer = String::new();

    loop {
        let event = reader.read_event().map_err(|e| format!("invalid WebDAV answer: {e}"))?;
        let is_empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                buffer.clear();
                let name = element.local_name();
                match name.as_ref() {
                    "response" => current = Some(Resource::default()),
                    "propstat" => {
                        pending = Resource::default();
                        status_ok = true;
                    }
                    "href" if !in_resourcetype => field = Field::Href,
                    "getcontentlength" => field = Field::Length,
                    "getlastmodified" => field = Field::Modified,
                    "status" => field = Field::Status,
                    "resourcetype" => in_resourcetype = true,
                    "collection" if in_resourcetype => pending.is_collection = true,
                    _ => {}
                }
                // An empty element has no end event: close it now.
                if is_empty {
                    match name.as_ref() {
                        "resourcetype" => in_resourcetype = false,
                        "response" => resources.extend(current.take()),
                        _ => {}
                    }
                    field = Field::None;
                }
            }
            Event::Text(text) => buffer.push_str(&text.xml10_content()),
            // `&amp;`, `&#233;`: the reader reports references separately.
            Event::GeneralRef(reference) => {
                let raw = format!("&{};", reference.xml10_content());
                let text = quick_xml::escape::unescape(&raw)
                    .map_err(|e| format!("invalid WebDAV answer: {e}"))?;
                buffer.push_str(&text);
            }
            Event::End(element) => {
                let text = std::mem::take(&mut buffer);
                let text = text.trim();
                match field {
                    Field::Href => {
                        if let Some(resource) = current.as_mut() {
                            resource.href = percent_decode(text);
                        }
                    }
                    Field::Length => pending.length = text.parse().ok(),
                    Field::Modified => pending.modified = httpdate::parse_http_date(text).ok(),
                    // "HTTP/1.1 200 OK"
                    Field::Status => {
                        let code = text.split_whitespace().nth(1);
                        status_ok = code.is_some_and(|c| c.starts_with('2'));
                    }
                    Field::None => {}
                }
                field = Field::None;
                match element.local_name().as_ref() {
                    "resourcetype" => in_resourcetype = false,
                    "propstat" => {
                        if let (true, Some(resource)) = (status_ok, current.as_mut()) {
                            resource.is_collection |= pending.is_collection;
                            resource.length = resource.length.or(pending.length);
                            resource.modified = resource.modified.or(pending.modified);
                        }
                    }
                    "response" => resources.extend(current.take()),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(resources)
}

fn percent_decode(text: &str) -> String {
    percent_encoding::percent_decode_str(text).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    const APACHE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:lp1="DAV:">
<D:response>
  <D:href>/dav/My%20Docs/</D:href>
  <D:propstat><D:prop>
    <lp1:resourcetype><D:collection/></lp1:resourcetype>
    <lp1:getlastmodified>Sat, 14 Nov 2026 10:00:00 GMT</lp1:getlastmodified>
  </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  <D:propstat><D:prop><D:getcontentlength/></D:prop>
    <D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
</D:response>
<D:response>
  <D:href>http://host/dav/My%20Docs/caf%C3%A9.txt</D:href>
  <D:propstat><D:prop>
    <lp1:resourcetype/>
    <lp1:getcontentlength>1234</lp1:getcontentlength>
    <lp1:getlastmodified>Thu, 01 Jan 1970 00:16:40 GMT</lp1:getlastmodified>
  </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
</D:multistatus>"#;

    #[test]
    fn reads_collections_and_files() {
        let resources = parse_multistatus(APACHE).unwrap();
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].href, "/dav/My Docs/");
        assert!(resources[0].is_collection);
        assert_eq!(resources[0].length, None, "a 404 propstat is ignored");
        assert_eq!(resources[1].href, "http://host/dav/My Docs/café.txt");
        assert!(!resources[1].is_collection);
        assert_eq!(resources[1].length, Some(1234));
        assert_eq!(resources[1].modified, Some(UNIX_EPOCH + Duration::from_secs(1000)));
    }

    #[test]
    fn prefixes_do_not_matter() {
        let xml = r#"<multistatus xmlns="DAV:"><response><href>/a</href>
            <propstat><prop><resourcetype><collection/></resourcetype></prop>
            <status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
        let resources = parse_multistatus(xml).unwrap();
        assert_eq!(resources.len(), 1);
        assert!(resources[0].is_collection);
    }

    #[test]
    fn broken_documents_are_errors() {
        assert!(parse_multistatus("<a><b></a>").is_err());
    }
}
