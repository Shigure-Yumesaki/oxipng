use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};
use crc::crc32;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

#[derive(Debug)]
pub enum ColorType {
    Grayscale,
    RGB,
    Indexed,
    GrayscaleAlpha,
    RGBA,
}

impl fmt::Display for ColorType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "{}",
               match *self {
                   ColorType::Grayscale => "Grayscale",
                   ColorType::RGB => "RGB",
                   ColorType::Indexed => "Indexed",
                   ColorType::GrayscaleAlpha => "Grayscale + Alpha",
                   ColorType::RGBA => "RGB + Alpha",
               })
    }
}

#[derive(Debug)]
pub enum BitDepth {
    One,
    Two,
    Four,
    Eight,
    Sixteen,
}

impl fmt::Display for BitDepth {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "{}",
               match *self {
                   BitDepth::One => "1",
                   BitDepth::Two => "2",
                   BitDepth::Four => "4",
                   BitDepth::Eight => "8",
                   BitDepth::Sixteen => "16",
               })
    }
}

#[derive(Debug)]
pub struct PngData {
    pub idat_data: Vec<u8>,
    pub ihdr_data: IhdrData,
    pub raw_data: Vec<u8>,
    pub palette: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct IhdrData {
    pub width: u32,
    pub height: u32,
    pub color_type: ColorType,
    pub bit_depth: BitDepth,
    pub compression: u8,
    pub filter: u8,
    pub interlaced: u8,
}

impl PngData {
    pub fn new(filepath: &Path) -> Result<PngData, String> {
        let mut file = match File::open(filepath) {
            Ok(f) => f,
            Err(_) => return Err("Failed to open file for reading".to_owned()),
        };
        let mut byte_data: Vec<u8> = Vec::new();
        // Read raw png data into memory
        match file.read_to_end(&mut byte_data) {
            Ok(_) => (),
            Err(_) => return Err("Failed to read from file".to_owned()),
        }
        let mut byte_offset: usize = 0;
        // Test that png header is valid
        let header: Vec<u8> = byte_data.iter().take(8).cloned().collect();
        if !file_header_is_valid(header.as_ref()) {
            return Err("Invalid PNG header detected".to_owned());
        }
        byte_offset += 8;
        // Read the data headers
        let mut aux_headers: HashMap<String, Vec<u8>> = HashMap::new();
        let mut idat_headers: Vec<u8> = Vec::new();
        loop {
            let header = parse_next_header(byte_data.as_ref(), &mut byte_offset);
            let header = match header {
                Ok(x) => x,
                Err(x) => return Err(x),
            };
            let header = match header {
                Some(x) => x,
                None => break,
            };
            if header.0 == "IDAT" {
                idat_headers.extend(header.1);
            } else {
                aux_headers.insert(header.0, header.1);
            }
        }
        // Parse the headers into our PngData
        if idat_headers.is_empty() {
            return Err("Image data was empty, skipping".to_owned());
        }
        if aux_headers.get("IHDR").is_none() {
            return Err("Image header data was missing, skipping".to_owned());
        }
        let ihdr_header = parse_ihdr_header(aux_headers.get("IHDR").unwrap().as_ref());
        let raw_data = match super::deflate::deflate::inflate(idat_headers.as_ref()) {
            Ok(x) => x,
            Err(x) => return Err(x),
        };
        // TODO: Reverse filtering?
        // Return the PngData
        Ok(PngData {
            idat_data: idat_headers.clone(),
            ihdr_data: match ihdr_header {
                Ok(x) => x,
                Err(x) => return Err(x),
            },
            raw_data: raw_data,
            palette: aux_headers.get("PLTE").cloned(),
        })
    }
    pub fn bits_per_pixel(&self) -> u8 {
        match self.ihdr_data.color_type {
            ColorType::Grayscale => 1,
            ColorType::RGB => 3,
            ColorType::Indexed => 1,
            ColorType::GrayscaleAlpha => 2,
            ColorType::RGBA => 4,
        }
    }
}

fn file_header_is_valid(bytes: &[u8]) -> bool {
    let expected_header: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    bytes.iter().zip(expected_header.iter()).all(|x| x.0 == x.1)
}

fn parse_next_header(byte_data: &[u8],
                     byte_offset: &mut usize)
                     -> Result<Option<(String, Vec<u8>)>, String> {
    let mut rdr = Cursor::new(byte_data.iter()
                                       .skip(*byte_offset)
                                       .take(4)
                                       .cloned()
                                       .collect::<Vec<u8>>());
    let length: u32 = match rdr.read_u32::<BigEndian>() {
        Ok(x) => x,
        Err(_) => return Err("Invalid data found--unable to read PNG file".to_owned()),
    };
    *byte_offset += 4;

    let mut header_bytes: Vec<u8> = byte_data.iter().skip(*byte_offset).take(4).cloned().collect();
    let header = match String::from_utf8(header_bytes.clone()) {
        Ok(x) => x,
        Err(_) => return Err("Invalid data found--unable to read PNG file".to_owned()),
    };
    if header == "IEND" {
        // End of data
        return Ok(None);
    }
    *byte_offset += 4;

    let data: Vec<u8> = byte_data.iter()
                                 .skip(*byte_offset)
                                 .take(length as usize)
                                 .cloned()
                                 .collect();
    *byte_offset += length as usize;
    let mut rdr = Cursor::new(byte_data.iter()
                                       .skip(*byte_offset)
                                       .take(4)
                                       .cloned()
                                       .collect::<Vec<u8>>());
    let crc: u32 = match rdr.read_u32::<BigEndian>() {
        Ok(x) => x,
        Err(_) => return Err("Invalid data found--unable to read PNG file".to_owned()),
    };
    *byte_offset += 4;
    header_bytes.extend(data.clone());
    if crc32::checksum_ieee(header_bytes.as_ref()) != crc {
        return Err(format!("Corrupt data chunk found--CRC Mismatch in {}", header));
    }

    Ok(Some((header, data)))
}

fn parse_ihdr_header(byte_data: &[u8]) -> Result<IhdrData, String> {
    let mut rdr = Cursor::new(&byte_data[0..8]);
    Ok(IhdrData {
        color_type: match byte_data[9] {
            0 => ColorType::Grayscale,
            2 => ColorType::RGB,
            3 => ColorType::Indexed,
            4 => ColorType::GrayscaleAlpha,
            6 => ColorType::RGBA,
            _ => return Err("Unexpected color type in header".to_owned()),
        },
        bit_depth: match byte_data[8] {
            1 => BitDepth::One,
            2 => BitDepth::Two,
            4 => BitDepth::Four,
            8 => BitDepth::Eight,
            16 => BitDepth::Sixteen,
            _ => return Err("Unexpected bit depth in header".to_owned()),
        },
        width: rdr.read_u32::<BigEndian>().unwrap(),
        height: rdr.read_u32::<BigEndian>().unwrap(),
        compression: byte_data[10],
        filter: byte_data[11],
        interlaced: byte_data[12],
    })
}