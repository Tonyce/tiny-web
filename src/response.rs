use std::cmp::Ordering;
use std::sync::mpsc::Receiver;

use std::io::Result as IoResult;
use std::io::{self, Cursor, Read, Write};

use std::fs::File;

use std::str::FromStr;

use crate::common::{HTTPDate, HTTPVersion, Header, StatusCode};

pub struct Response<R>
where
    R: Read,
{
    reader: R,
    status_code: StatusCode,
    headers: Vec<Header>,
    data_length: Option<usize>,
    chunked_threshold: Option<usize>,
}

pub type ResponseBox = Response<Box<dyn Read + Send>>;

#[derive(Copy, Clone)]
enum TransferEncoding {
    Identity,
    Chunked,
}

impl FromStr for TransferEncoding {
    type Err = ();
    fn from_str(input: &str) -> Result<TransferEncoding, ()> {
        if input.eq_ignore_ascii_case("identity") {
            Ok(TransferEncoding::Identity)
        } else if input.eq_ignore_ascii_case("chunked") {
            Ok(TransferEncoding::Chunked)
        } else {
            Err(())
        }
    }
}

fn build_data_header() -> Header {
    let d = HTTPDate::new();
    Header::from_bytes(&b"Date"[..], &d.to_string().into_bytes()[..]).unwrap()
}

fn write_message_header<W>(
    mut writer: W,
    http_version: &HTTPVersion,
    status_code: &StatusCode,
    headers: &[Header],
) -> IoResult<()>
where
    W: Write,
{
    write!(
        &mut writer,
        "HTTP/{}.{} {} {}\r\n",
        http_version.0,
        http_version.1,
        status_code.0,
        status_code.default_reason_phrase()
    )?;

    for header in headers.iter() {
        writer.write_all(header.field.as_str().as_ref())?;
        write!(&mut writer, ": ")?;
        writer.write_all(header.value.as_str().as_ref())?;
        write!(&mut writer, "\r\n")?;
    }
    write!(&mut writer, "\r\n")?;
    Ok(())
}

fn choose_transfer_encoding(
    request_headers: &[Header],
    http_version: &HTTPVersion,
    entity_length: &Option<usize>,
    has_additional_headers: bool,
    chunked_threshold: usize,
) -> TransferEncoding {
    use crate::util;

    if *http_version <= (1, 0) {
        return TransferEncoding::Identity;
    }

    let user_request = request_headers
        .iter()
        .find(|h| h.field.equiv(&"TE"))
        .map(|h| h.value.clone())
        .and_then(|value| {
            let mut parse = util::parse_header_value(value.as_str());
            parse.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            for value in parse.iter() {
                if value.1 <= 0.0 {
                    continue;
                }
                match <TransferEncoding as FromStr>::from_str(value.0) {
                    Ok(te) => return Some(te),
                    _ => (),
                };
            }
            None
        });

    if let Some(user_request) = user_request {
        return user_request;
    }
    if has_additional_headers {
        return TransferEncoding::Chunked;
    }
    if entity_length
        .as_ref()
        .map_or(true, |val| *val >= chunked_threshold)
    {
        return TransferEncoding::Chunked;
    }
    TransferEncoding::Identity
}

impl<R> Response<R>
where
    R: Read,
{
    pub fn new(
        status_code: StatusCode,
        headers: Vec<Header>,
        data: R,
        data_length: Option<usize>,
        additional_headers: Option<Receiver<Header>>,
    ) -> Response<R> {
        let mut response = Response {
            reader: data,
            status_code,
            headers: Vec::with_capacity(16),
            data_length,
            chunked_threshold: None,
        };
        for h in headers {
            response.add_header(h)
        }

        if let Some(additional_headers) = additional_headers {
            for h in additional_headers.iter() {
                response.add_header(h)
            }
        }
        response
    }

    pub fn with_chunked_threshold(mut self, length: usize) -> Response<R> {
        self.chunked_threshold = Some(length);
        self
    }

    pub fn chunked_threshold(&self) -> usize {
        self.chunked_threshold.unwrap_or(32768) // ?
    }

    pub fn add_header<H>(&mut self, header: H)
    where
        H: Into<Header>,
    {
        let header = header.into();
        // ignoring forbidden headers
        if header.field.equiv(&"Accept-Ranges")
            || header.field.equiv(&"Connection")
            || header.field.equiv(&"Content-Range")
            || header.field.equiv(&"Trailer")
            || header.field.equiv(&"Transfer-Encoding")
            || header.field.equiv(&"Upgrade")
        {
            return;
        }

        if header.field.equiv(&"Content-Length") {
            match <usize as FromStr>::from_str(header.value.as_str()) {
                Ok(val) => self.data_length = Some(val),
                Err(_) => (), // wrong value for content-length
            };

            return;
        }

        self.headers.push(header);
    }

    /// Returns the same request, but with an additional header.
    ///
    /// Some headers cannot be modified and some other have a
    ///  special behavior. See the documentation above.
    #[inline]
    pub fn with_header<H>(mut self, header: H) -> Response<R>
    where
        H: Into<Header>,
    {
        self.add_header(header.into());
        self
    }

    /// Returns the same request, but with a different status code.
    #[inline]
    pub fn with_status_code<S>(mut self, code: S) -> Response<R>
    where
        S: Into<StatusCode>,
    {
        self.status_code = code.into();
        self
    }

    /// Returns the same request, but with different data.
    pub fn with_data<S>(self, reader: S, data_length: Option<usize>) -> Response<S>
    where
        S: Read,
    {
        Response {
            reader: reader,
            headers: self.headers,
            status_code: self.status_code,
            data_length: data_length,
            chunked_threshold: None,
        }
    }

    pub fn raw_print<W: Write>(
        mut self,
        mut writer: W,
        http_version: HTTPVersion,
        request_headers: &[Header],
        do_not_send_body: bool,
        upgrade: Option<&str>,
    ) -> IoResult<()> {
        let mut transfer_encoding = Some(choose_transfer_encoding(
            request_headers,
            &http_version,
            &self.data_length,
            false,
            self.chunked_threshold(),
        ));

        if self
            .headers
            .iter()
            .find(|h| h.field.equiv(&"Date"))
            .is_none()
        {
            self.headers.insert(0, build_data_header());
        }

        if self
            .headers
            .iter()
            .find(|h| h.field.equiv(&"Server"))
            .is_none()
        {
            self.headers.insert(
                0,
                Header::from_bytes(&b"Server"[..], &b"tiny-http (Rust)"[..]).unwrap(),
            );
        }

        if let Some(upgrade) = upgrade {
            self.headers.insert(
                0,
                Header::from_bytes(&b"Upgrade"[..], upgrade.as_bytes()).unwrap(),
            );
            self.headers.insert(
                0,
                Header::from_bytes(&b"Connection"[..], &b"upgrade"[..]).unwrap(),
            );
            transfer_encoding = None;
        }

        let (mut reader, data_length) = match (self.data_length, transfer_encoding) {
            (Some(l), _) => (Box::new(self.reader) as Box<dyn Read>, Some(l)),
            (None, Some(TransferEncoding::Identity)) => {
                let mut buf = Vec::new();
                self.reader.read_to_end(&mut buf)?;
                let l = buf.len();
                (Box::new(Cursor::new(buf)) as Box<dyn Read>, Some(l))
            }
            _ => (Box::new(self.reader) as Box<dyn Read>, None),
        };

        let do_not_send_body = do_not_send_body
            || match self.status_code.0 {
                100..=199 | 204 | 304 => true,
                _ => false,
            };

        match transfer_encoding {
            Some(TransferEncoding::Chunked) => self
                .headers
                .push(Header::from_bytes(&b"Transfer-Encoding"[..], &b"chunked"[..]).unwrap()),
            Some(TransferEncoding::Identity) => {
                assert!(data_length.is_some());
                let data_length = data_length.unwrap();
                self.headers.push(
                    Header::from_bytes(
                        &b"Content-Length"[..],
                        format!("{}", data_length).as_bytes(),
                    )
                    .unwrap(),
                )
            }
            _ => (),
        }

        write_message_header(
            writer.by_ref(),
            &http_version,
            &self.status_code,
            &self.headers,
        )?;

        if !do_not_send_body {
            match transfer_encoding {
                Some(TransferEncoding::Chunked) => {
                    use chunked_transfer::Encoder;
                    let mut writer = Encoder::new(writer);
                    io::copy(&mut reader, &mut writer)?;
                }
                Some(TransferEncoding::Identity) => {
                    use crate::util::EqualReader;
                    assert!(data_length.is_some());
                    let data_length = data_length.unwrap();
                    if data_length >= 1 {
                        let (mut equ_reader, _) = EqualReader::new(reader.by_ref(), data_length);
                        io::copy(&mut equ_reader, &mut writer)?;
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
}

impl<R> Response<R>
where
    R: Read + Send + 'static,
{
    pub fn boxed(self) -> ResponseBox {
        Response {
            reader: Box::new(self.reader) as Box<dyn Read + Send>,
            status_code: self.status_code,
            headers: self.headers,
            data_length: self.data_length,
            chunked_threshold: None,
        }
    }
}

impl Response<File> {
    pub fn from_file(file: File) -> Response<File> {
        let file_size = file.metadata().ok().map(|v| v.len() as usize);
        Response::new(
            StatusCode(200),
            Vec::with_capacity(0),
            file,
            file_size,
            None,
        )
    }
}

impl Response<Cursor<Vec<u8>>> {
    pub fn from_data<D>(data: D) -> Response<Cursor<Vec<u8>>>
    where
        D: Into<Vec<u8>>,
    {
        let data = data.into();
        let data_len = data.len();

        Response::new(
            StatusCode(200),
            Vec::with_capacity(0),
            Cursor::new(data),
            Some(data_len),
            None,
        )
    }

    pub fn from_string<S>(data: S) -> Response<Cursor<Vec<u8>>>
    where
        S: Into<String>,
    {
        let data = data.into();
        let data_len = data.len();

        Response::new(
            StatusCode(200),
            vec![
                Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=UTF-8"[..])
                    .unwrap(),
            ],
            Cursor::new(data.into_bytes()),
            Some(data_len),
            None,
        )
    }
}

impl Response<io::Empty> {
    /// Builds an empty `Response` with the given status code.
    pub fn empty<S>(status_code: S) -> Response<io::Empty>
    where
        S: Into<StatusCode>,
    {
        Response::new(
            status_code.into(),
            Vec::with_capacity(0),
            io::empty(),
            Some(0),
            None,
        )
    }

    /// DEPRECATED. Use `empty` instead.
    pub fn new_empty(status_code: StatusCode) -> Response<io::Empty> {
        Response::empty(status_code)
    }
}

impl Clone for Response<io::Empty> {
    fn clone(&self) -> Response<io::Empty> {
        Response {
            reader: io::empty(),
            status_code: self.status_code.clone(),
            headers: self.headers.clone(),
            data_length: self.data_length.clone(),
            chunked_threshold: self.chunked_threshold.clone(),
        }
    }
}
