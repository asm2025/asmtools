use csv::{ReaderBuilder, WriterBuilder};
pub use serde::{de, Deserialize, Serialize};
pub use serde_json;
use std::{error::Error, fs};
pub use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Result as ioResult, Write},
    path::Path,
};

use super::directory;

const LINES_BUFFER_DEFAULT: usize = 1000;

pub enum FileOpenOptions {
    Default,
    New,
    Truncate,
    Append,
}

pub fn exists<T: AsRef<Path>>(path: T) -> bool {
    let path = path.as_ref();
    path.exists() && path.is_file()
}

pub fn open<T: AsRef<Path>>(path: T) -> ioResult<std::fs::File> {
    let mut opt = OpenOptions::new();
    opt.read(true);
    from_options(path, &opt)
}

pub fn create<T: AsRef<Path>>(
    path: T,
    options: Option<FileOpenOptions>,
) -> ioResult<std::fs::File> {
    let path = path.as_ref();
    let dir = path.parent().unwrap();
    directory::ensure(dir)?;

    let mut opt = OpenOptions::new();
    opt.read(true);
    match options {
        Some(FileOpenOptions::New) => opt.create_new(true),
        Some(FileOpenOptions::Truncate) => opt.create(true).truncate(true),
        Some(FileOpenOptions::Append) => opt.create(true).append(true),
        _ => opt.create(true),
    };
    opt.write(true);
    from_options(path, &opt)
}

pub fn from_options<T: AsRef<Path>>(path: T, options: &OpenOptions) -> ioResult<std::fs::File> {
    let path = path.as_ref();
    options.open(path)
}

pub fn delete<T: AsRef<Path>>(path: T) -> ioResult<()> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)
}

pub trait FileEx {
    fn read<'a>(&'a self) -> ioResult<impl Iterator<Item = String> + 'a>;
    fn read_filtered<'a, F: Fn(&str) -> bool + 'static>(
        &'a self,
        filter: F,
    ) -> ioResult<impl Iterator<Item = String> + 'a>;
    fn read_batch<'a, R: Fn(u32, Vec<String>) -> bool + 'static>(
        &'a self,
        batch: usize,
        callback: R,
    ) -> ioResult<u32>;
    fn read_batch_filtered<
        'a,
        F: Fn(&str) -> bool + 'static,
        R: Fn(u32, Vec<String>) -> bool + 'static,
    >(
        &'a self,
        batch: usize,
        filter: F,
        callback: R,
    ) -> ioResult<u32>;
    fn write<'a, T: AsRef<str>>(&'a mut self, data: &'a T) -> ioResult<()>;
    fn write_lines<'a, T: AsRef<str>>(&'a mut self, data: impl Iterator<Item = T>) -> ioResult<()>;
    fn read_json<'a, T: de::DeserializeOwned>(&'a self) -> Result<T, Box<dyn Error>>;
    fn write_json<'a, T: Serialize>(
        &'a mut self,
        data: &'a T,
        pretty: Option<bool>,
    ) -> Result<(), Box<dyn Error>>;
    fn create_delimited_reader<'a>(
        &'a mut self,
        delimiter: Option<u8>,
        has_headers: Option<bool>,
    ) -> csv::Reader<&'a mut std::fs::File>;
    fn create_delimited_writer<'a>(
        &'a mut self,
        delimiter: Option<u8>,
        has_headers: Option<bool>,
    ) -> csv::Writer<&'a mut std::fs::File>;
}

impl FileEx for std::fs::File {
    fn read(&self) -> ioResult<impl Iterator<Item = String>> {
        let reader = BufReader::new(self);
        Ok(reader
            .lines()
            .filter_map(|line| line.ok())
            .filter(|line| !line.is_empty()))
    }

    fn read_filtered<'a, F: Fn(&str) -> bool + 'static>(
        &'a self,
        filter: F,
    ) -> ioResult<impl Iterator<Item = String> + 'a> {
        let reader = BufReader::new(self);
        Ok(reader
            .lines()
            .filter_map(|line| line.ok())
            .filter(move |line| !line.is_empty() && filter(line)))
    }

    fn read_batch<'a, R: Fn(u32, Vec<String>) -> bool + 'static>(
        &'a self,
        batch: usize,
        callback: R,
    ) -> ioResult<u32> {
        let batch = if batch == 0 {
            LINES_BUFFER_DEFAULT
        } else {
            batch
        };
        let mut reader = BufReader::new(self);
        let mut batch_number = 0u32;
        let mut line: String = String::new();
        let mut lines = Vec::with_capacity(batch);

        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }

            if line.is_empty() {
                line.clear();
                continue;
            }

            lines.push(line.clone());
            line.clear();

            if lines.len() < batch {
                continue;
            }

            batch_number += 1;
            let result = lines.clone();
            lines.clear();

            if !callback(batch_number, result) {
                break;
            }
        }

        if lines.is_empty() {
            return Ok(batch_number);
        }

        batch_number += 1;
        callback(batch_number, lines);
        Ok(batch_number)
    }

    fn read_batch_filtered<
        'a,
        F: Fn(&str) -> bool + 'static,
        R: Fn(u32, Vec<String>) -> bool + 'static,
    >(
        &'a self,
        batch: usize,
        filter: F,
        callback: R,
    ) -> ioResult<u32> {
        let batch = if batch == 0 {
            LINES_BUFFER_DEFAULT
        } else {
            batch
        };
        let mut reader = BufReader::new(self);
        let mut batch_number = 0u32;
        let mut line: String = String::new();
        let mut lines = Vec::with_capacity(batch);

        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }

            if line.is_empty() || !filter(&line) {
                line.clear();
                continue;
            }

            lines.push(line.clone());
            line.clear();

            if lines.len() < batch {
                continue;
            }

            batch_number += 1;
            let result = lines.clone();
            lines.clear();

            if !callback(batch_number, result) {
                break;
            }
        }

        if lines.is_empty() {
            return Ok(batch_number);
        }

        batch_number += 1;
        callback(batch_number, lines);
        Ok(batch_number)
    }

    fn write<'a, T: AsRef<str>>(&'a mut self, data: &'a T) -> ioResult<()> {
        writeln!(self, "{}", data.as_ref())
    }

    fn write_lines<'a, T: AsRef<str>>(&'a mut self, data: impl Iterator<Item = T>) -> ioResult<()> {
        for line in data.into_iter() {
            writeln!(self, "{}", line.as_ref())?;
        }

        Ok(())
    }

    fn read_json<'a, T: de::DeserializeOwned>(&'a self) -> Result<T, Box<dyn Error>> {
        let reader = BufReader::new(self);
        let data: T = serde_json::from_reader(reader)?;
        Ok(data)
    }

    fn write_json<'a, T: Serialize>(
        &'a mut self,
        data: &'a T,
        pretty: Option<bool>,
    ) -> Result<(), Box<dyn Error>> {
        let serialize = match pretty {
            Some(true) => serde_json::to_string_pretty,
            _ => serde_json::to_string,
        };
        let serialized = serialize(data)?;
        self.write_all(serialized.as_bytes())?;
        Ok(())
    }

    fn create_delimited_reader<'a>(
        &'a mut self,
        delimiter: Option<u8>,
        has_headers: Option<bool>,
    ) -> csv::Reader<&'a mut std::fs::File> {
        let delimiter = delimiter.unwrap_or(b',');
        let has_headers = has_headers.unwrap_or(false);
        ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(has_headers)
            .from_reader(self)
    }

    fn create_delimited_writer<'a>(
        &'a mut self,
        delimiter: Option<u8>,
        has_headers: Option<bool>,
    ) -> csv::Writer<&'a mut std::fs::File> {
        let delimiter = delimiter.unwrap_or(b',');
        let has_headers = has_headers.unwrap_or(false);
        WriterBuilder::new()
            .delimiter(delimiter)
            .has_headers(has_headers)
            .from_writer(self)
    }
}
