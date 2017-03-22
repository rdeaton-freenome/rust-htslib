use std::ffi;
use std::path::Path;

use htslib;

use bam::header;
use bam::record;
use bam::Reader;
use bam::Read;

// new on bam::HeaderView is not public
pub struct SAMHeaderView {
    inner: *mut htslib::bam_hdr_t,
    owned: bool,
}


impl SAMHeaderView {
    fn new(inner: *mut htslib::bam_hdr_t) -> Self {
        SAMHeaderView { 
            inner: inner,
            owned: true,
        }
    }
    #[inline]
    fn inner(&self) -> htslib::bam_hdr_t {
        unsafe { (*self.inner) }
    }
}


/// SAM writer.
pub struct SAMWriter {
    f: *mut htslib::htsFile,
    header: SAMHeaderView,
}

/// Wrapper for opening a SAM file.
fn hts_open(path: &ffi::CStr, mode: &[u8]) -> Result<*mut htslib::htsFile, SAMError> {
    let ret = unsafe {
        htslib::hts_open(
            path.as_ptr(),
            ffi::CString::new(mode).unwrap().as_ptr()
        )
    };
    if ret.is_null() {
        Err(SAMError::IOError)
    } else {
        Ok(ret)
    }
}

impl SAMWriter {
    /// Create new SAM file writer.
    ///
    /// # Arguments
    ///
    /// * `path` - the path.
    /// * `header` - header definition to use
    pub fn from_path<P: AsRef<Path>>(path: P, header: &header::Header) -> Result<Self, SAMError> {
        if let Some(p) = path.as_ref().to_str() {
            Ok(try!(Self::new(p.as_bytes(), header)))
        } else {
            Err(SAMError::IOError)
        }
    }

    /// Create a new SAM file at STDOUT.
    ///
    /// # Arguments
    ///
    /// * `header` - header definition to use
    pub fn from_stdout(header: &header::Header) -> Result<Self, SAMError> {
        Self::new(b"-", header)
    }

    fn new(path: &[u8], header: &header::Header) -> Result<Self, SAMError> {
        let f = try!(hts_open(&ffi::CString::new(path).unwrap(), b"w"));
        let header_record = unsafe {
            let header_string = header.to_bytes();
            let l_text = header_string.len();
            let text = ::libc::malloc(l_text + 1);
            ::libc::memset(text, 0, l_text + 1);
            ::libc::memcpy(text, header_string.as_ptr() as *const ::libc::c_void, header_string.len());
            //println!("{}", std::str::from_utf8(&header_string).unwrap());
            let rec = htslib::sam_hdr_parse(
                (l_text + 1) as i32,
                text as *const i8,
            );
            (*rec).text = text as *mut i8;
            (*rec).l_text = l_text as u32;
            rec
        };
        unsafe { htslib::sam_hdr_write(f, header_record); }
        Ok(SAMWriter { f: f, header: SAMHeaderView::new(header_record) })
    }

    /// Write record to SAM.
    ///
    /// # Arguments
    ///
    /// * `record` - the record to write
    pub fn write(&mut self, record: &record::Record) -> Result<(), WriteError> {
        if unsafe { htslib::sam_write1(self.f, &self.header.inner(), record.inner) } == -1 {
            Err(WriteError::Some)
        }
        else {
            Ok(())
        }
    }

    /// Read bam file. For each record apply f to it, and write to sam file if f returned Some(true), skip record if Some(false) if None then terminate iteration
    ///
    /// # Arguments
    ///
    /// * `bamfile` - the bam file to read from
    /// * `samfile` - the sam file to write
    /// * `f` - the predicate to apply
    pub fn from_bam_with_filter<'a, 'b, F>(bamfile:&'a str, samfile:&'b str, f:F) -> Result<(), SAMError> where F:Fn(&record::Record) -> Option<bool> {
        let bam_reader = if bamfile != "-" {
            match Reader::from_path(bamfile) {
                Ok(bam) => bam,
                Err(_) => return Err(SAMError::IOError)
            }
        } else {
            match Reader::from_stdin() {
                Ok(bam) => bam,
                Err(_) => return Err(SAMError::IOError)
            }

        };
        let header = header::Header::from_template(bam_reader.header());
        let mut sam_writer = if samfile != "-" {
                SAMWriter::from_path(samfile, &header)?
            } else {
                SAMWriter::from_stdout(&header)?
            };
        for record in bam_reader.records() {
            if record.is_err() {
                return Err(SAMError::IOError)
            } 
            let parsed = record.unwrap();
            match f(&parsed) {
                None => return Ok(()),
                Some(false) => {},
                Some(true) => if let Err(_) = sam_writer.write(&parsed) {
                    return Err(SAMError::IOError);
                }
            }
        }
        Ok(())
    }

}

impl Drop for SAMWriter {
    fn drop(&mut self) {
        unsafe {
            htslib::hts_close(self.f);
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum SAMError {
        IOError {}
    }
}

#[test]
fn test_sam_writer_example() {
    fn from_bam_with_filter<'a, 'b, F>(bamfile:&'a str, samfile:&'b str, f:F) -> bool where F:Fn(&record::Record) -> Option<bool> {
        let bam_reader = Reader::from_path(bamfile).unwrap(); // internal functions, just unwarp
        let header = header::Header::from_template(bam_reader.header());
        let mut sam_writer = SAMWriter::from_path(samfile, &header).unwrap();
        for record in bam_reader.records() {
            if record.is_err() {
                return false;
            } 
            let parsed = record.unwrap();
            match f(&parsed) {
                None => return true,
                Some(false) => {},
                Some(true) => if let Err(_) = sam_writer.write(&parsed) {
                    return false;
                }
            }
        }
        true
    }
    use std::fs::File;
    use std::io::Read;
    let bamfile = "./test/bam2sam_test.bam";
    let samfile = "./test/bam2sam_out.sam";
    let expectedfile = "./test/bam2sam_expected.sam";
    let result = from_bam_with_filter(bamfile, samfile, |_|{Some(true)});
    assert!(result);
    let mut expected = Vec::new();
    let mut written = Vec::new();
    assert!(File::open(expectedfile).unwrap().read_to_end(&mut expected).is_ok()); 
    assert!(File::open(samfile).unwrap().read_to_end(&mut written).is_ok());
    assert_eq!(expected, written);
}

quick_error! {
    #[derive(Debug)]
    pub enum WriteError {
        Some {
            description("error writing record")
        }
    }
}


