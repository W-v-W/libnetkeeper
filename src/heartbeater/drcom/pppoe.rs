use std::{marker, io};
use std::net::Ipv4Addr;
use std::num::Wrapping;

use byteorder::{NativeEndian, NetworkEndian, ByteOrder};

use crypto::hash::{HasherBuilder, Hasher, HasherType};
use common::reader::{ReadBytesError, ReaderHelper};
use common::drcom::{DrCOMCommon, DrCOMResponseCommon};

#[derive(Debug)]
pub enum HeartbeatFlag {
    First,
    NotFirst,
}

#[derive(Debug)]
pub enum CRCHasherType {
    NONE,
    MD5,
    MD4,
    SHA1,
}

#[derive(Debug)]
pub enum CRCHashError {
    ModeNotExist,
    InputLengthInvalid,
}

struct NoneHasher;

#[derive(Debug)]
pub struct ChallengeRequest {
    sequence: u8,
}

#[derive(Debug)]
pub struct ChallengeResponse {
    pub challenge_seed: u32,
    pub source_ip: Ipv4Addr,
}

#[derive(Debug)]
pub struct HeartbeatRequest {
    sequence: u8,
    type_id: u8,
    uid_length: u8,
    mac_address: [u8; 6],
    source_ip: Ipv4Addr,
    flag: HeartbeatFlag,
    challenge_seed: u32,
}

trait CRCHasher {
    fn hasher(&self) -> Box<Hasher>;
    fn retain_postions(&self) -> [usize; 8];

    fn hash(&self, bytes: &[u8]) -> [u8; 8] {
        let mut hasher = self.hasher();
        let retain_postions = self.retain_postions();

        hasher.update(bytes);
        let hashed_bytes = hasher.finish();

        let mut hashed = Vec::<u8>::with_capacity(retain_postions.len());
        for i in &retain_postions {
            if *i > hashed_bytes.len() {
                continue;
            }
            hashed.push(hashed_bytes[*i]);
        }

        let mut result = [0u8; 8];
        result.clone_from_slice(hashed.as_slice());
        result
    }
}

trait CRCHasherBuilder {
    fn from_mode(mode: u8) -> Result<Self, CRCHashError> where Self: marker::Sized;
}


impl Hasher for NoneHasher {
    #[allow(unused_variables)]
    fn update(&mut self, bytes: &[u8]) {}
    fn finish(&mut self) -> Vec<u8> {
        const DRCOM_DIAL_EXT_PROTO_CRC_INIT: u32 = 20000711;
        let mut result = vec![0u8; 8];
        NativeEndian::write_u32(result.as_mut_slice(), DRCOM_DIAL_EXT_PROTO_CRC_INIT);
        NativeEndian::write_u32(&mut result.as_mut_slice()[4..], 126);
        result
    }
}

impl CRCHasher for CRCHasherType {
    fn hasher(&self) -> Box<Hasher> {
        match *self {
            CRCHasherType::NONE => Box::new(NoneHasher {}) as Box<Hasher>,
            CRCHasherType::MD5 => HasherBuilder::build(HasherType::MD5),
            CRCHasherType::MD4 => HasherBuilder::build(HasherType::MD4),
            CRCHasherType::SHA1 => HasherBuilder::build(HasherType::SHA1),
        }
    }

    fn retain_postions(&self) -> [usize; 8] {
        match *self {
            CRCHasherType::NONE => [0, 1, 2, 3, 4, 5, 6, 7],
            CRCHasherType::MD5 => [2, 3, 8, 9, 5, 6, 13, 14],
            CRCHasherType::MD4 => [1, 2, 8, 9, 4, 5, 11, 12],
            CRCHasherType::SHA1 => [2, 3, 9, 10, 5, 6, 15, 16],
        }
    }
}

impl CRCHasherBuilder for CRCHasherType {
    fn from_mode(mode: u8) -> Result<Self, CRCHashError>
        where Self: marker::Sized
    {
        match mode {
            0 => Ok(CRCHasherType::NONE),
            1 => Ok(CRCHasherType::MD5),
            2 => Ok(CRCHasherType::MD4),
            3 => Ok(CRCHasherType::SHA1),

            _ => Err(CRCHashError::ModeNotExist),
        }
    }
}

impl DrCOMCommon for ChallengeRequest {}
impl DrCOMResponseCommon for ChallengeResponse {}

impl ChallengeRequest {
    pub fn new(sequence: Option<u8>) -> Self {
        let sequence = match sequence {
            Some(c) => c,
            None => 1u8,
        };
        ChallengeRequest { sequence: sequence }
    }

    fn magic_number() -> u32 {
        65544u32
    }

    pub fn as_bytes(&self) -> [u8; 8] {
        let mut result = [0u8; 8];
        result[0] = Self::code();
        result[1] = self.sequence;
        NativeEndian::write_u32(&mut result[2..], Self::magic_number());
        result
    }
}

impl ChallengeResponse {
    pub fn from_bytes<R>(input: &mut io::BufReader<R>) -> Result<Self, ReadBytesError>
        where R: io::Read
    {
        // validate packet and consume 1 byte
        try!(Self::validate_packet(input));
        // drain unknow bytes
        try!(input.read_bytes(7));

        let challenge_seed;
        {
            let challenge_seed_bytes = try!(input.read_bytes(4));
            challenge_seed = NativeEndian::read_u32(&challenge_seed_bytes);
        }

        let source_ip;
        {
            let source_ip_bytes = try!(input.read_bytes(4));
            source_ip = Ipv4Addr::from(NetworkEndian::read_u32(&source_ip_bytes));
        }

        Ok(ChallengeResponse {
            challenge_seed: challenge_seed,
            source_ip: source_ip,
        })
    }
}

impl HeartbeatFlag {
    fn as_u32(&self) -> u32 {
        match *self {
            HeartbeatFlag::First => 0x2a006200u32,
            HeartbeatFlag::NotFirst => 0x2a006300u32,
        }
    }
}

impl DrCOMCommon for HeartbeatRequest {}

impl HeartbeatRequest {
    pub fn new(sequence: u8,
               source_ip: Ipv4Addr,
               flag: HeartbeatFlag,
               challenge_seed: u32,
               type_id: Option<u8>,
               uid_length: Option<u8>,
               mac_address: Option<[u8; 6]>)
               -> Self {
        let type_id = match type_id {
            Some(tid) => tid,
            None => 3u8,
        };
        let uid_length = match uid_length {
            Some(ul) => ul,
            None => 0u8,
        };
        let mac_address = match mac_address {
            Some(mac) => mac,
            None => [0u8; 6],
        };
        HeartbeatRequest {
            sequence: sequence,
            type_id: type_id,
            uid_length: uid_length,
            mac_address: mac_address,
            source_ip: source_ip,
            flag: flag,
            challenge_seed: challenge_seed,
        }
    }

    fn header_length() -> usize {
        1 + // code 
        1 + // sequence
        2 // packet_length
    }

    fn content_length() -> usize {
        1 + // type_id
        1 + // uid_length
        6 + // mac_address
        4 + // source_ip
        4 + // pppoe_flag
        4 // challenge_seed
    }

    fn footer_length() -> usize {
        8 + // crc_hash
        16 * 4 // padding?
    }

    fn packet_length() -> usize {
        Self::header_length() + Self::content_length() + Self::footer_length()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut header_bytes = Vec::with_capacity(Self::header_length());
        {
            header_bytes.push(Self::code());
            header_bytes.push(self.sequence);

            let mut packet_length_bytes = [0u8; 2];
            {
                NativeEndian::write_u16(&mut packet_length_bytes, Self::packet_length() as u16);
            }
            header_bytes.extend_from_slice(&packet_length_bytes);
        }

        let mut challenge_seed_bytes = [0u8; 4];
        {
            NativeEndian::write_u32(&mut challenge_seed_bytes, self.challenge_seed);
        }

        let mut content_bytes = Vec::with_capacity(Self::content_length());
        {
            content_bytes.push(self.type_id);
            content_bytes.push(self.uid_length);
            content_bytes.extend_from_slice(&self.mac_address);
            content_bytes.extend_from_slice(&self.source_ip.octets());

            let mut flag_bytes = [0u8; 4];
            {
                NativeEndian::write_u32(&mut flag_bytes, self.flag.as_u32());
            }
            content_bytes.extend_from_slice(&flag_bytes);
            content_bytes.extend_from_slice(&challenge_seed_bytes);
        }

        let mut footer_bytes = Vec::with_capacity(Self::footer_length());
        {
            let hash_mode = CRCHasherType::from_mode((self.challenge_seed % 3) as u8).unwrap();
            let crc_hash_bytes = hash_mode.hash(&challenge_seed_bytes);
            footer_bytes.extend_from_slice(&crc_hash_bytes);

            if let CRCHasherType::NONE = hash_mode {
                let mut rehash_bytes: Vec<u8> = Vec::with_capacity(Self::packet_length());
                rehash_bytes.extend(&header_bytes);
                rehash_bytes.extend(&content_bytes);
                rehash_bytes.extend(&footer_bytes);
                let rehash = Wrapping(calculate_drcom_crc32(&rehash_bytes, None).unwrap()) *
                             Wrapping(19680126);
                NativeEndian::write_u32(&mut footer_bytes, rehash.0);
                NativeEndian::write_u32(&mut footer_bytes[4..], 0u32);
            }
            // padding?
            footer_bytes.extend_from_slice(&[0u8; 16 * 4]);
        }

        let mut packet_bytes = Vec::with_capacity(Self::packet_length());
        packet_bytes.extend(header_bytes);
        packet_bytes.extend(content_bytes);
        packet_bytes.extend(footer_bytes);
        packet_bytes
    }
}

fn calculate_drcom_crc32(bytes: &[u8], initial: Option<u32>) -> Result<u32, CRCHashError> {
    if bytes.len() % 4 != 0 {
        return Err(CRCHashError::InputLengthInvalid);
    }

    let mut result = match initial {
        Some(initial) => initial,
        None => 0,
    };
    for c in 0..(bytes.len() / 4usize) {
        result ^= NativeEndian::read_u32(&bytes[c * 4..c * 4 + 4]);
    }
    Ok(result)
}

#[test]
fn test_generate_crc_hash() {
    let crc_hash_none = CRCHasherType::NONE.hash(b"1234567890");
    let crc_hash_md5 = CRCHasherType::MD5.hash(b"1234567890");
    let crc_hash_md4 = CRCHasherType::MD4.hash(b"1234567890");
    let crc_hash_sha1 = CRCHasherType::SHA1.hash(b"1234567890");
    assert_eq!(crc_hash_md5, [241, 252, 155, 176, 45, 19, 56, 161]);
    assert_eq!(crc_hash_sha1, [7, 172, 175, 195, 79, 84, 246, 202]);
    assert_eq!(crc_hash_none, [199, 47, 49, 1, 126, 0, 0, 0]);
    assert_eq!(crc_hash_md4, [177, 150, 28, 171, 227, 148, 144, 95]);
}

#[test]
fn test_calculate_drcom_crc32() {
    let crc32 = calculate_drcom_crc32(b"1234567899999999", None).unwrap();
    assert_eq!(crc32, 201589764);
}
