pub use ethers::types::{
    transaction::eip2718::TypedTransaction,
    transaction::eip1559::Eip1559TransactionRequest,
    transaction::eip2930::{AccessList, AccessListItem},
    TransactionRequest, NameOrAddress, Address, U256, Bytes, Signature, H256, U64
};

pub use ethers::core::k256::{
    ecdsa::{Signature as K256Signature, SigningKey, VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint,
};

use revm::{
    primitives::{AuthorizationList, BlockEnv, SpecId, TxEnv, ruint::Uint},
    Handler,
};

pub use ethers::utils::keccak256;

pub use hex::FromHex;
pub use rlp::{Rlp, RlpStream};
pub use std::env;
pub use super::adapter;

use std::fs::File;
use std::io::{self, BufRead, BufReader};

#[derive(Debug)]
pub enum DecodedTransaction {
    Legacy(TransactionRequest, Address),
    Eip1559(Eip1559TransactionRequest, Address),
    Unknown(String),
}

fn u256_from_bytes(b: &[u8]) -> U256 {
    if b.is_empty() {
        U256::from(0)
    } else {
        U256::from_big_endian(b)
    }
}

fn u64_from_bytes(b: &[u8]) -> U64 {
    if b.is_empty() {
        U64::from(0)
    } else {
        U64::from_big_endian(b)
    }
}



fn parse_to_address(b: &[u8]) -> Option<Address> {
    if b.is_empty() {
        None
    } else {
        // RLP Address is 20 bytes
        if b.len() != 20 {
            // The last 20 bytes are the address
            if b.len() > 20 {
                let mut a = [0u8; 20];
                a.copy_from_slice(&b[b.len() - 20..]);
                Some(Address::from(a))
            } else {
                None
            }
        } else {
            let mut a = [0u8; 20];
            a.copy_from_slice(b);
            Some(Address::from(a))
        }
    }
}


fn hexify(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// Parse the access list from RLP
fn parse_access_list(rlp: &rlp::Rlp, idx: usize) -> Vec<(ethers::types::Address, Vec<ethers::types::H256>)> {
    let mut out = Vec::new();

    // accessList is a list of tuples: [address, [storageKeys...]]
    let access_list_rlp = rlp.at(idx).expect("accessList rlp");

    for item in access_list_rlp.iter() {
        // item = [ address, [storageKeys...] ]
        let addr_bytes = item.at(0).unwrap().data().unwrap_or_default();
        let addr = {
            use ethers::types::Address;
            if addr_bytes.len() == 20 {
                let mut a = [0u8; 20];
                a.copy_from_slice(addr_bytes);
                Address::from(a)
            } else {
                // The last 20 bytes are the address
                let mut a = [0u8; 20];
                let start = addr_bytes.len().saturating_sub(20);
                a.copy_from_slice(&addr_bytes[start..]);
                Address::from(a)
            }
        };

        // List of storageKeys 
        let mut keys = Vec::new();
        let storage_rlp = item.at(1).unwrap();
        for k in storage_rlp.iter() {
            let kb = k.data().unwrap_or_default();
            let mut arr = [0u8; 32];
            if kb.len() <= 32 {
                arr[32 - kb.len()..].copy_from_slice(kb);
                keys.push(ethers::types::H256::from(arr));
            }
        }

        out.push((addr, keys));
    }

    out
}

fn recover_address(
    r_sig: U256,
    s_sig: U256,
    v: U256,
    unsigned: Vec<u8>,
) -> Result<Address, Box<dyn std::error::Error>> {
    let sig = Signature {
        r: r_sig,
        s: s_sig,
        v: v.as_u64(),
    };

    let sighash = ethers::utils::keccak256(&unsigned);
    let recovered_addr = sig.recover(sighash).expect("recover failed");
    println!("Recovered from: {:?}", recovered_addr);
    Ok(recovered_addr)
}


fn decode_legacy(bytes: &[u8], no_signature: Vec<u8>) -> (TransactionRequest, Address) {
    let r = Rlp::new(bytes);

    let nonce = u256_from_bytes(r.at(0).unwrap().data().unwrap_or_default());
    let gas_price = u256_from_bytes(r.at(1).unwrap().data().unwrap_or_default());
    let gas_limit = u256_from_bytes(r.at(2).unwrap().data().unwrap_or_default());
    let to_b = r.at(3).unwrap().data().unwrap_or_default();
    let to = parse_to_address(to_b);
    let value = u256_from_bytes(r.at(4).unwrap().data().unwrap_or_default());
    let data = r.at(5).unwrap().data().unwrap_or_default();

    let mut caller = Address::default();

    println!("# Legacy (type 0)");
    println!("nonce                : {}", nonce);
    println!("gasPrice (wei)       : {}", gas_price);
    println!("gasLimit             : {}", gas_limit);
    println!("to                   : {}", to.map(|a| format!("{a:?}")).unwrap_or_else(|| "<create>".into()));
    println!("value (wei)          : {}", value);
    println!("data                 : {}", hexify(data));

    if r.item_count().unwrap_or(0) >= 9 {
        let v = u256_from_bytes(r.at(6).unwrap().data().unwrap_or_default());
        let r_sig = u256_from_bytes(r.at(7).unwrap().data().unwrap_or_default());
        let s_sig = u256_from_bytes(r.at(8).unwrap().data().unwrap_or_default());
        println!("v                    : {}", v);
        println!("r                    : 0x{:064x}", r_sig);
        println!("s                    : 0x{:064x}", s_sig);

        caller = recover_address(r_sig, s_sig, v, no_signature).expect("Failed to recover address");
    }

    // Construct LegacyTransactionRequest
    (TransactionRequest {
        from: Some(caller),
        to: to.map(NameOrAddress::Address),
        gas: Some(gas_limit),
        gas_price: Some(gas_price),
        value: Some(value),
        data: Some(Bytes::from(data.to_vec())),
        nonce: Some(nonce),
        ..Default::default()
    }, caller)

}

// fn decode_eip2930(inner: &[u8], no_signature: Vec<u8>) {
//     // RLP: [chainId, nonce, gasPrice, gasLimit, to, value, data, accessList, v?, r?, s?]
//     let r = Rlp::new(inner);
//     let chain_id = u256_from_bytes(r.at(0).unwrap().data().unwrap_or_default());
//     let nonce = u256_from_bytes(r.at(1).unwrap().data().unwrap_or_default());
//     let gas_price = u256_from_bytes(r.at(2).unwrap().data().unwrap_or_default());
//     let gas_limit = u256_from_bytes(r.at(3).unwrap().data().unwrap_or_default());
//     let to = parse_to_address(r.at(4).unwrap().data().unwrap_or_default());
//     let value = u256_from_bytes(r.at(5).unwrap().data().unwrap_or_default());
//     let data = r.at(6).unwrap().data().unwrap_or_default();
//     let access_list = parse_access_list(&r, 7);
    

//     println!("# EIP-2930 (type 0x01)");
//     println!("chainId              : {}", chain_id);
//     println!("nonce                : {}", nonce);
//     println!("gasPrice (wei)       : {}", gas_price);
//     println!("gasLimit             : {}", gas_limit);
//     println!("to                   : {}", to.map(|a| format!("{a:?}")).unwrap_or_else(|| "<create>".into()));
//     println!("value (wei)          : {}", value);
//     println!("data                 : {}", hexify(data));
//     println!("accessList           : [{} items]", access_list.len());

//     if r.item_count().unwrap_or(0) >= 10 {
//         let v = u256_from_bytes(r.at(8).unwrap().data().unwrap_or_default());
//         let r_sig = u256_from_bytes(r.at(9).unwrap().data().unwrap_or_default());
//         let s_sig = u256_from_bytes(r.at(10).unwrap().data().unwrap_or_default());
//         println!("v                    : {}", v);
//         println!("r                    : 0x{:064x}", r_sig);
//         println!("s                    : 0x{:064x}", s_sig);

//         let sig = Signature {
//             r: r_sig,
//             s: s_sig,
//             v: v.as_u64(),
//         };

//         recover_address(r_sig, s_sig, v, no_signature).expect("Failed to recover address");
//     }
// }

fn u256_to_h256(val: U256) -> H256 {
    let mut bytes = [0u8; 32];
    val.to_big_endian(&mut bytes);
    H256::from(bytes)
}

fn decode_eip1559(inner: &[u8], no_signature: Vec<u8>) -> (Eip1559TransactionRequest, Address) {
    // RLP: [chainId, nonce, maxPriorityFeePerGas, maxFeePerGas, gasLimit, to, value, data, accessList, v?, r?, s?]
    let r = Rlp::new(inner);
    let chain_id = u64_from_bytes(r.at(0).unwrap().data().unwrap_or_default());
    let nonce = u256_from_bytes(r.at(1).unwrap().data().unwrap_or_default());
    let max_priority = u256_from_bytes(r.at(2).unwrap().data().unwrap_or_default());
    let max_fee = u256_from_bytes(r.at(3).unwrap().data().unwrap_or_default());
    let gas_limit = u256_from_bytes(r.at(4).unwrap().data().unwrap_or_default());
    let to = parse_to_address(r.at(5).unwrap().data().unwrap_or_default());
    let value = u256_from_bytes(r.at(6).unwrap().data().unwrap_or_default());
    let data = r.at(7).unwrap().data().unwrap_or_default();
    let access_list = parse_access_list(&r, 8);

    let mut caller = Address::default();

    // println!("# EIP-1559 (type 0x02)");
    // println!("chainId              : {}", chain_id);
    // println!("nonce                : {}", nonce);
    // println!("maxPriorityFeePerGas : {}", max_priority);
    // println!("maxFeePerGas         : {}", max_fee);
    // println!("gasLimit             : {}", gas_limit);
    // println!("to                   : {}", to.map(|a| format!("{a:?}")).unwrap_or_else(|| "<create>".into()));
    // println!("value (wei)          : {}", value);
    // println!("data                 : {}", hexify(data));
    // println!("accessList           : [{} items]", access_list.len());
    
    // Recover sender address if available
    if r.item_count().unwrap_or(0) >= 11 {
        let v = u256_from_bytes(r.at(9).unwrap().data().unwrap_or_default());
        let r_sig = u256_from_bytes(r.at(10).unwrap().data().unwrap_or_default());
        let s_sig = u256_from_bytes(r.at(11).unwrap().data().unwrap_or_default());
        // println!("v                    : {}", v);
        // println!("r                    : 0x{:064x}", r_sig);
        // println!("s                    : 0x{:064x}", s_sig);
        
        caller = recover_address(r_sig, s_sig, v, no_signature).expect("Failed to recover address");
    }

    let access_list = AccessList::from(access_list.into_iter()
    .map(|(address, storage_keys)| AccessListItem { address, storage_keys })
    .collect::<Vec<AccessListItem>>());

    (Eip1559TransactionRequest {
        from: Some(caller),
        chain_id: Some(chain_id),
        nonce: Some(nonce),
        max_priority_fee_per_gas: Some(max_priority),
        max_fee_per_gas: Some(max_fee),
        gas: Some(gas_limit),
        to: to.map(NameOrAddress::Address),
        value: Some(value),
        data: Some(Bytes::from(data.to_vec())),
        access_list: access_list,
        ..Default::default()
    }, caller)
}

fn strip_signature(raw_bytes: &[u8]) -> Vec<u8> {
    // Check if the first byte indicates a typed transaction (EIP-2718)
    let (tx_type, rlp_bytes) = if !raw_bytes.is_empty() && (raw_bytes[0] == 0x02 || raw_bytes[0] == 0x01) {
        (Some(raw_bytes[0]), &raw_bytes[1..])
    } else {
        (None, raw_bytes)
    };

    // Decode RLP
    let rlp = Rlp::new(rlp_bytes);
    let total_items = rlp.item_count().expect("invalid RLP");

    if total_items < 3 {
        panic!("Not enough fields to strip v/r/s");
    }

    // Remove v, r, s from the RLP
    let mut stream = RlpStream::new_list(total_items - 3);
    for i in 0..(total_items - 3) {
        stream.append_raw(rlp.at(i).unwrap().as_raw(), 1);
    }

    // Add the tx type if it exists
    let mut out = Vec::new();
    if let Some(t) = tx_type {
        out.push(t);
    }
    out.extend(stream.out().to_vec());
    out
}


pub fn decode_hex(raw_hex: &str) -> DecodedTransaction {
    // Remove "0x" prefix if present
    let h = raw_hex.trim_start_matches("0x");
    let bytes = Vec::from_hex(h).expect("invalid hex");

    if bytes.is_empty() {
        eprintln!("empty bytes");
        return DecodedTransaction::Unknown(String::from("Error: Empty Bytes"))
    }

    let no_signature = strip_signature(bytes.as_slice());
    // println!("raw bytes without signature: {}", hexify(&no_signature));

    match bytes[0] {
        // 0x01 => {
        //     // EIP-2930
        //     if bytes.len() < 2 {
        //         eprintln!("malformed 2930");
        //         return;
        //     }
        //     let eip2930 = decode_eip2930(&bytes[1..], no_signature);
        //     // eip2930
        // }
        0x02 => {
            // EIP-1559
            if bytes.len() < 2 {
                eprintln!("malformed 1559");
                return DecodedTransaction::Unknown(String::from("Error: malformed 1559"))
            }
            let (eip1559, caller) = decode_eip1559(&bytes[1..], no_signature);
            DecodedTransaction::Eip1559(eip1559, caller)
        }
        b if b >= 0xc0 => {
            // Legacy
            let (legacy, caller) = decode_legacy(&bytes, no_signature);
            DecodedTransaction::Legacy(legacy, caller)
        }
        _ => {
            DecodedTransaction::Unknown(String::from("Error: Unknown transaction type"))
        }
    }
}


pub fn decode_hex_with_known_addr(raw_hex: &str, addr: Address) -> DecodedTransaction {
    // Remove "0x" prefix if present
    let h = raw_hex.trim_start_matches("0x");
    let bytes = Vec::from_hex(h).expect("invalid hex");

    if bytes.is_empty() {
        eprintln!("empty bytes");
        return DecodedTransaction::Unknown(String::from("Error: Empty Bytes"))
    }

    let no_signature = strip_signature(bytes.as_slice());

    match bytes[0] {
        0x02 => {
            // EIP-1559
            if bytes.len() < 2 {
                eprintln!("malformed 1559");
                return DecodedTransaction::Unknown(String::from("Error: malformed 1559"))
            }
            let (eip1559, caller) = decode_eip1559(&bytes[1..], no_signature);
            DecodedTransaction::Eip1559(eip1559, addr)
        }
        b if b >= 0xc0 => {
            // Legacy
            let (legacy, caller) = decode_legacy(&bytes, no_signature);
            DecodedTransaction::Legacy(legacy, addr)
        }
        _ => {
            DecodedTransaction::Unknown(String::from("Error: Unknown transaction type"))
        }
    }
}


pub fn read_from_file(file_path: &str) -> io::Result<Vec<(String, Address)>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut results = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            eprintln!("Skipping malformed line: {}", line);
            continue;
        }

        let hexcode = parts[0].to_string();
        let address: Address = parts[1].parse().expect("Invalid Ethereum address");

        results.push((hexcode, address));
    }

    Ok(results)
}

pub fn decode_one_hex(
    hex: String,
    addr: Address,
) -> Option<TxEnv> {
    let deserialized_tx = decode_hex_with_known_addr(&hex, addr);
    match deserialized_tx {
        DecodedTransaction::Legacy(_, _) => {
            let tx_env = adapter::adapt_transaction(deserialized_tx);
            Some(tx_env)
        },
        DecodedTransaction::Eip1559(_, _) => {
            let tx_env = adapter::adapt_transaction(deserialized_tx);
            Some(tx_env)
        },
        _ => {
            eprintln!("Skipping unsupported transaction type for hex: {}", hex);
            None
        }
    }
}


pub fn decode_batch_hex(
    hex_address: Vec<(String, Address)>,
) -> Vec<TxEnv> {
    let mut results = Vec::new();
    for (hex, addr) in hex_address {
        let deserialized_tx = decode_hex_with_known_addr(&hex, addr);
        match deserialized_tx {
            DecodedTransaction::Legacy(_, _) => {
                let tx_env = adapter::adapt_transaction(deserialized_tx);
                results.push(tx_env);
            },
            DecodedTransaction::Eip1559(_, _) => {
                let tx_env = adapter::adapt_transaction(deserialized_tx);
                results.push(tx_env);
            },
            _ => {
                eprintln!("Skipping unsupported transaction type for hex: {}", hex);
                continue;
            }
        }
    }

    results
}


pub struct ChunkFileReader {
    reader: BufReader<File>,
}

impl ChunkFileReader {
    /// Open a file for reading
    pub fn open(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
        })
    }

    /// Read the next `n` lines from the file, returning a vector of (hexcode, address) tuples.
    /// Start from the current position in the file.
    pub fn read_next(&mut self, n: usize) -> io::Result<Vec<(String, Address)>> {
        let mut out = Vec::with_capacity(n);
        let mut buf = String::new();

        while out.len() < n {
            buf.clear();
            let bytes = self.reader.read_line(&mut buf)?;
            if bytes == 0 {
                // EOF
                break;
            }

            let line = buf.trim();
            if line.is_empty() {
                continue;
            }

            let mut parts = line.split_whitespace();
            let hexcode = match parts.next() {
                Some(s) => s,
                None => {
                    eprintln!("Skipping malformed line: {}", line);
                    continue;
                }
            };
            let addr_str = match parts.next() {
                Some(s) => s,
                None => {
                    eprintln!("Skipping malformed line: {}", line);
                    continue;
                }
            };
            
            if parts.next().is_some() {
                eprintln!("Skipping malformed line: {}", line);
                continue;
            }

            match addr_str.parse::<Address>() {
                Ok(addr) => out.push((hexcode.to_string(), addr)),
                Err(_) => {
                    eprintln!("Invalid Ethereum address: {}", addr_str);
                }
            }
        }

        Ok(out)
    }
}