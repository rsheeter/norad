//! Customize serialization behaviour

use std::{borrow::Cow, fs::File, io::BufWriter, path::Path};

#[cfg(target_family = "unix")]
use std::os::unix::prelude::FileExt;

#[cfg(target_family = "windows")]
use std::os::windows::prelude::*;

use plist::XmlWriteOptions;

/// Options that can be set when writing the UFO to disk.
///
/// You construct `WriteOptions` using builder semantics:
///
/// ```
/// # use norad::{QuoteChar, WriteOptions};
/// let single_tab = WriteOptions::default();
///
/// let two_tabs = WriteOptions::default()
///     .whitespace("\t\t");
///
/// let spaces = WriteOptions::default()
///     .whitespace("  ");
///
/// let spaces_and_singlequotes = WriteOptions::default()
///     .whitespace("  ")
///     .quote_char(QuoteChar::Single);
/// ```
#[derive(Debug, Clone)]
pub struct WriteOptions {
    // for annoying reasons we store three different representations.
    pub(crate) indent_str: Cow<'static, str>,
    xml_opts: XmlWriteOptions,
    pub(crate) whitespace_char: u8,
    pub(crate) whitespace_count: usize,
    pub(crate) quote_style: QuoteChar,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            indent_str: "\t".into(),
            xml_opts: Default::default(),
            whitespace_char: b'\t',
            whitespace_count: 1,
            quote_style: QuoteChar::Double,
        }
    }
}

impl WriteOptions {
    /// Builder-style method to customize the whitespace.
    ///
    /// By default, we indent with a single tab ("\t").
    ///
    /// The argument, may be either a `'static str` or a `String`. You should
    /// prefer to use a `'static str` where possible.
    ///
    /// The string can contain any number of *a single ASCII character*, but must
    /// not contain multiple different characters. As an example, "\t\t" is
    /// fine, but "\t  \t" is not, because it contains both tabs and spaces.
    ///
    // This is not good API, but is a work around for the fact that the quick-xml
    // and plist crates both represent whitespace in different ways.
    ///
    /// # Panics
    ///
    /// Panics if the provided string is empty, or if it contains multiple
    /// different characters.
    pub fn whitespace(mut self, indent_str: impl Into<Cow<'static, str>>) -> Self {
        let indent_str = indent_str.into();
        self.whitespace_char = indent_str.bytes().next().expect("whitespace str must not be empty");
        assert!(indent_str.bytes().all(|c| c == self.whitespace_char), "invalid whitespace");
        self.whitespace_count = indent_str.len();
        self.indent_str = indent_str;
        self.xml_opts = XmlWriteOptions::default().indent_string(self.indent_str.clone());
        self
    }

    /// Builder-style method to customize the XML declaration attribute definition quote
    /// char.
    ///
    /// By default, we indent with double quotes.
    ///
    /// The quote style is defined with a [`QuoteChar`] enum argument.
    pub fn quote_char(mut self, quote_style: QuoteChar) -> Self {
        self.quote_style = quote_style;
        self
    }

    /// Return a reference to [`XmlWriteOptions`] for use with the `plist` crate.
    pub fn xml_options(&self) -> &XmlWriteOptions {
        &self.xml_opts
    }
}

/// The quote character used to write the XML declaration.
///
/// This is exposed to allow the user to match the output of other tools.
#[derive(Debug, Clone)]
pub enum QuoteChar {
    /// Single quotes: 'UTF-8'.
    Single,
    /// Double quotes: "UTF-8".
    Double,
}

/// Write any `Serialize` to file, providing custom options.
pub(crate) fn write_xml_to_file(
    path: &Path,
    value: &impl serde::Serialize,
    options: &WriteOptions,
) -> Result<(), CustomSerializationError> {
    let mut file = File::create(path).map_err(CustomSerializationError::CreateFile)?;
    let buf_writer = BufWriter::new(&mut file);
    plist::to_writer_xml_with_options(buf_writer, value, options.xml_options())
        .map_err(CustomSerializationError::SerializePlist)?;
    write_quote_style(&file, options).map_err(CustomSerializationError::WriteQuotes)?;
    file.sync_all().map_err(CustomSerializationError::Sync)?;
    Ok(())
}

/// Write XML declarations with custom quote formatting options.
fn write_quote_style(file: &File, options: &WriteOptions) -> Result<(), std::io::Error> {
    // Optionally modify the XML declaration quote style
    match options.quote_style {
        QuoteChar::Single => {
            // Unix platform specific write
            #[cfg(target_family = "unix")]
            file.write_at(b"<?xml version='1.0' encoding='UTF-8'?>", 0)?;
            // Windows platform specific write
            #[cfg(target_family = "windows")]
            file.seek_write(b"<?xml version='1.0' encoding='UTF-8'?>", 0)?;
        }
        QuoteChar::Double => (), // double quote is the default style
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CustomSerializationError {
    #[error("failed to create file")]
    CreateFile(#[source] std::io::Error),
    #[error("failed to serialize Plist")]
    SerializePlist(#[source] plist::Error),
    #[error("failed to rewrite quote style")]
    WriteQuotes(#[source] std::io::Error),
    #[error("failed to sync file to disk")]
    Sync(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::Value;
    use std::fs;
    use tempdir::TempDir;

    #[test]
    fn write_lib_plist_default() {
        let opt = WriteOptions::default();
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/lib.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("lib.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"); // default uses double quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "\t<key>com.defcon.sortDescriptor</key>"); // single tab spacing by default
        assert_eq!(str_list[6], "\t\t<dict>"); // second level should use two tab char
        tmp.close().unwrap();
    }

    #[test]
    fn write_lib_plist_with_custom_whitespace() {
        let opt = WriteOptions::default().whitespace("  ");
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/lib.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("lib.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"); // default uses double quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "  <key>com.defcon.sortDescriptor</key>"); // should use two space char
        assert_eq!(str_list[6], "    <dict>"); // second level should use four space char
        tmp.close().unwrap();
    }

    #[test]
    fn write_lib_plist_with_custom_whitespace_and_single_quotes() {
        let opt = WriteOptions::default().whitespace("  ").quote_char(QuoteChar::Single);
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/lib.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("lib.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version='1.0' encoding='UTF-8'?>"); // should use single quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "  <key>com.defcon.sortDescriptor</key>"); // should use two space char
        assert_eq!(str_list[6], "    <dict>"); // second level should use four space char
        tmp.close().unwrap();
    }

    #[test]
    fn write_lib_plist_line_endings() {
        let opt = WriteOptions::default();
        let plist_read = Value::from_file("testdata/lineendings/Tester-LineEndings.ufo/lib.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("lib.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        assert!(plist_write.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"));
        tmp.close().unwrap();
    }

    #[test]
    fn write_fontinfo_plist_default() {
        let opt = WriteOptions::default();
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/fontinfo.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("fontinfo.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"); // default uses double quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "\t<key>ascender</key>"); // single tab level spacing by default
        tmp.close().unwrap();
    }

    #[test]
    fn write_fontinfo_plist_with_custom_whitespace() {
        let opt = WriteOptions::default().whitespace("  ");
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/fontinfo.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("fontinfo.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"); // default uses double quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "  <key>ascender</key>"); // should use two space char
        tmp.close().unwrap();
    }

    #[test]
    fn write_fontinfo_plist_with_custom_whitespace_and_single_quotes() {
        let opt = WriteOptions::default().whitespace("  ").quote_char(QuoteChar::Single);
        let plist_read = Value::from_file("testdata/MutatorSansLightWide.ufo/fontinfo.plist")
            .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("fontinfo.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        let str_list = plist_write.split('\n').collect::<Vec<&str>>();
        assert_eq!(str_list[0], "<?xml version='1.0' encoding='UTF-8'?>"); // should use single quotes
        assert_eq!(str_list[3], "<dict>"); // no space char at first dict tag
        assert_eq!(str_list[4], "  <key>ascender</key>"); // should use two space char
        tmp.close().unwrap();
    }

    #[test]
    fn write_fontinfo_plist_line_endings() {
        let opt = WriteOptions::default();
        let plist_read =
            Value::from_file("testdata/lineendings/Tester-LineEndings.ufo/fontinfo.plist")
                .expect("failed to read plist");
        let tmp = TempDir::new("test").unwrap();
        let filepath = tmp.path().join("fontinfo.plist");
        write_xml_to_file(&filepath, &plist_read, &opt).unwrap();
        let plist_write = fs::read_to_string(filepath).unwrap();
        assert!(plist_write.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"));
        tmp.close().unwrap();
    }
}
